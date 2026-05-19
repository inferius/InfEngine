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

use std::collections::{HashMap, HashSet, VecDeque};
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use glam::{Mat4, Vec3};
use infengine_editor::EditorUi;
use infengine_net::{NetClient, NetServer};
use infengine_render::{MeshHandle, MeshVertex, Renderer};
use infengine_voxel::{
    BlockDef, BlockId, BlockRegistry, CHUNK_SIZE, Chunk, ChunkCoord, ChunkSubmesh, RayHit, World,
    chunk_mesh_with_atlas, raycast_with,
};
use infengine_window::raw_winit::event::{ElementState, MouseButton, WindowEvent};
use infengine_window::{AppHandler, EngineWindow, FrameContext, Key, WindowConfig, run};

mod block;
use block::{
    ATLAS_COLS, ATLAS_ROWS, BLOCK_TYPES, BUCKET_EMPTY_ID, BUCKET_LAVA_ID,
    BUCKET_WATER_ID, BlockCategory, CHEST_ID, FURNACE_ID, GLOWSTONE_ID, LAVA_ID,
    TILE_PX, TORCH_ID, TileSpec, WATER_ID, atlas_uv_rect, axe_tier,
    block_category, block_is_aabb_passable, block_is_liquid,
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
    ClientInv, DamageMsg, DropSpawnMsg, EditMsg, InvActionMsg,
    InvSnapshotMsg, MobEventMsg, MobHitMsg, MobMsg, NetRole,
    PlayerMsg, PlayerRemoveMsg, PvPHitMsg, TeleportMsg,
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
mod dim_switch_tests;
#[cfg(test)]
mod dim_teleport_tests;
#[cfg(test)]
mod debug_tests;
mod mining;
mod mob_ai;
mod player;
mod ui;
mod worldgen;
use chat::{CHAT_FADE_SECS, CHAT_HISTORY_MAX, CHAT_LOG_MAX, line_is_muted};
use mining::{MiningState, block_hardness};
use mob_ai::{
    KIND_CHICKEN, KIND_PIG, KIND_SHEEP, KIND_ZOMBIE, MOB_ATTACK_REACH,
    MOB_CAP_DEFAULT, MOB_DEATH_FADE, MOB_HIT_DAMAGE, MOB_LUNGE_TIME,
    MOB_MAX_HEALTH, MobState, ZOMBIE_MAX_HEALTH,
};
use player::{
    COLLISION_EPSILON, GRAVITY, JUMP_VELOCITY, PLAYER_HEIGHT, PLAYER_RADIUS,
    PLAYER_SPEED, aabb_intersects_solid, body_aabb, submerged_liquid,
    submerged_liquid_eye,
};
use ui::{AppState, BackTo, HighlightMode, InvAction, SlotIcon, match_recipe, match_recipe_named};
use worldgen::{
    Biome, WORLDGEN, WorldGen, biome_at, set_world_seed, world_seed,
};
use drops::{DROP_ATTRACT_RADIUS, DropItem};
use inventory::{
    HOTBAR_SLOTS, INVENTORY_COLS, INVENTORY_ROWS, INVENTORY_SLOTS,
    SlotKind, SplitMode, STACK_MAX, inv_apply_drag, inv_consume, inv_pickup,
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
// moved to `mob_ai.rs` and re-imported below.

const PVP_DAMAGE: f32 = 1.5;
const PVP_REACH: f32 = 5.5;

/// Initial chunk grid loaded eagerly before the player drops in. The
/// world streams more chunks in as the player walks beyond this
/// footprint.
const INITIAL_CHUNKS: i32 = 3;
/// Number of stacked chunk layers along the Y axis. Each chunk is
/// `CHUNK_SIZE` (=16) tall, so 4 layers give us 64 vertical voxels
/// - enough for mountains, sky-builds, and underground digging.
const VERTICAL_CHUNKS: i32 = 4;
/// Chunks within this Chebyshev distance of the player chunk are
/// kept loaded; new ones get queued for generation.
const VIEW_DISTANCE_CHUNKS: i32 = 4;
/// Maximum number of worker results consumed per frame (each runs
/// `Renderer::create_mesh` for several submeshes - bounded so the
/// main thread doesn't stall on a sudden burst).
const RESULTS_PER_FRAME: usize = 4;
/// Number of background worker threads generating + meshing chunks.
const NUM_WORKERS: usize = 2;
// `PLAYER_SPEED`, `JUMP_VELOCITY`, `GRAVITY`, `PLAYER_HEIGHT`,
// `PLAYER_RADIUS`, `COLLISION_EPSILON` moved to `player.rs` and
// re-imported below.

/// Damage source classifier - drives the per-kind hurt SFX
/// `apply_player_damage_kind` plays on integer heart loss. New
/// kinds drop in here + add a `match` arm in the SFX selector.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum DamageKind {
    /// Mob hits, fall damage, PvP - the standard hurt thud.
    Generic,
    /// Air ran out underwater - subtle bubble + airy swish.
    Drown,
    /// Hot liquid contact - sizzling swish, no bass.
    Burn,
}

/// Maximum player hit points. 20 mirrors the Minecraft scale (10
/// hearts of 2 HP each).
pub(crate) const PLAYER_MAX_HEALTH: f32 = 20.0;
const STAMINA_MAX: f32 = 100.0;
/// Breath capacity in seconds. MC matches this at ~15 s with
/// the default lung capacity. Drains 1 s/s while submerged in a
/// liquid that has `drains_air = true`; refills 4x as fast
/// after surfacing.
const AIR_MAX_SEC: f32 = 15.0;
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
        }
    }
}

struct Voxel {
    state: AppState,
    settings: Settings,
    requested_quit: bool,
    /// Wall-clock instant the app started. Subtracted each frame
    /// to produce `time_seconds` for the camera uniform so shaders
    /// (animated water) can drive procedural animation.
    app_start: std::time::Instant,

    renderer: Option<Renderer>,
    ui: Option<EditorUi>,
    registry: BlockRegistry,
    world: World,
    // Phase F per-dim fields. Read sites land in F.2 (multi-dim
    // save/load) + F.6 (cross-dim teleport); silence the
    // "field never read" warning so the F.1 commit is clean.
    /// Phase F: name of the currently-active dimension. Drives
    /// the chunks file path (`chunks/<current_dim>/...`), the
    /// per-dim seed (`dim_seed(base_seed, current_dim)`), the
    /// per-dim worldgen profile, and which entries in the
    /// `_by_dim` parking maps are "live" vs "parked".
    #[allow(dead_code)]
    current_dim: String,
    /// Base seed (matches `world.json::seed`). The effective
    /// `WORLD_SEED` atomic equals `dim_seed(base_seed, current_dim)`
    /// at any moment - rewritten on every `switch_dim` so noise
    /// callers stay deterministic per-dim.
    #[allow(dead_code)]
    base_seed: u32,
    /// Parking lot for inactive-dim voxel + side-state. The
    /// active dim's slot is empty - its data lives in the
    /// top-level fields (`world`, `chests`, `furnaces`, ...).
    #[allow(dead_code)]
    worlds: HashMap<String, dim::DimWorld>,
    /// Parking lot for inactive-dim mobs + drops. Mirrors
    /// `worlds` - the active dim's `mobs` / `dropped_items` live
    /// in the top-level fields.
    #[allow(dead_code)]
    entities_by_dim: HashMap<String, dim::DimEntities>,
    /// Per-dim worldgen variant lookup (mirrors
    /// `WorldJson::dimension_worldgens` so the runtime knows
    /// which `WorldGen` to pin when switching to a dim that
    /// hasn't been visited yet this session).
    #[allow(dead_code)]
    dim_worldgens: HashMap<String, WorldGen>,
    /// Phase F.6: ticks the player has been stood inside a
    /// portal cell continuously. Increments while the player
    /// AABB overlaps a `PORTAL_ID` cell; resets to 0 on step
    /// out. Once it reaches `PORTAL_TRIGGER_TICKS` the runtime
    /// teleports the player to the paired dim.
    #[allow(dead_code)]
    portal_in_ticks: u32,
    /// Phase F.6: cool-down counter set on teleport. Decremented
    /// each tick; while > 0 the portal-overlap check is
    /// suppressed so the player doesn't immediately bounce
    /// back through the destination portal.
    #[allow(dead_code)]
    portal_cooldown_ticks: u32,
    /// Mesh handles grouped by the chunk that produced them. Lets the
    /// streaming code rebuild / unload only the chunks that change.
    chunk_meshes: Vec<(ChunkCoord, BlockId, MeshHandle)>,
    /// Chunks queued for generation that haven't been dispatched to a
    /// worker yet.
    pending_chunks: VecDeque<ChunkCoord>,
    /// Set of chunks either queued OR currently being generated by a
    /// worker thread. Used to avoid re-dispatching the same coord.
    pending_set: HashSet<ChunkCoord>,
    /// Worker pool the streaming code dispatches generation to. CPU-
    /// heavy `generate_chunk` + `chunk_mesh` run on the workers; the
    /// main thread only does the GPU buffer upload + neighbour
    /// remeshing.
    workers: Option<WorkerPool>,
    /// Most recent ray hit while looking around in Play. Drives the
    /// crosshair / wireframe overlay and the RMB-place step. Skips
    /// `F_RAY_PASS` decorations (torch, flowers) so the player can
    /// place a block adjacent to a torch without targeting it.
    target: Option<RayHit>,
    /// Parallel ray that stops on EVERY non-air voxel, decorations
    /// included. Drives mining (LMB) so the torch / flower itself
    /// is breakable when the crosshair sits on it - the regular
    /// `target` ray would tunnel through and pick the block behind.
    mine_target: Option<RayHit>,

    player_pos: Vec3,
    player_vel: Vec3,
    player_yaw: f32,
    player_pitch: f32,

    // True while the OS reports this window as focused. Cursor
    // lock + mouse-motion handling are gated on it so two instances
    // on the same machine don't fight over input.
    has_focus: bool,
    last_mouse: Option<[f32; 2]>,
    mouse_dx: f32,
    mouse_dy: f32,
    esc_prev: bool,
    /// Previous-frame state of the E key. Drives the inventory-open
    /// toggle: only fires on the rising edge so holding E doesn't
    /// flicker the overlay.
    e_prev: bool,
    /// Previous-frame state of the F key. Rising edge consumes
    /// one piece of food from the inventory + restores HP.
    f_prev: bool,
    /// F1 toggles HUD visibility. Useful for clean screenshots
    /// and stress-testing the renderer without overlay overhead.
    hud_visible: bool,
    f1_prev: bool,
    /// True while the chat input box is open. Gates player input
    /// like the inventory / MP menu overlays.
    chat_open: bool,
    /// Buffer the chat input box edits while open.
    chat_input: String,
    /// Recent chat lines: `(arrival_instant, formatted_text)`.
    /// Older entries fade out + drop after a few seconds.
    chat_log: Vec<(std::time::Instant, String)>,
    /// Player's previously sent chat lines (most recent last).
    /// Used by Up/Down arrow scrolling in the chat input box.
    /// Capped at 20 entries so very long sessions don't grow it
    /// unbounded.
    chat_history: Vec<String>,
    /// Index of the currently-shown history entry, or `None` when
    /// the player is typing fresh text. Reset whenever a new line
    /// is sent.
    chat_history_idx: Option<usize>,
    /// Previous-frame state of the T key for rising-edge open.
    t_prev: bool,
    lmb_pending: bool,
    rmb_pending: bool,
    /// Level state of the mouse buttons (set on Pressed events,
    /// cleared on Released). Drives the hold-to-mine loop - the
    /// edge-triggered `lmb_pending` still routes single clicks to
    /// melee attacks on mobs.
    lmb_held: bool,
    rmb_held: bool,
    /// Active mining session. `None` when the player isn't holding
    /// LMB on a voxel; populated with the targeted cell + the time
    /// elapsed / required to break the block. Cancels when LMB
    /// releases, the aim moves to a different voxel, or the
    /// targeted voxel disappears (someone else broke it first).
    mining: Option<MiningState>,
    /// Current player hit points. Capped at `PLAYER_MAX_HEALTH`.
    /// Reaches zero = death, which respawns at origin + wipes the
    /// storage rows the same way the fall-out path already does.
    player_health: f32,
    /// World-Y the player last left solid ground at. `None` when
    /// grounded; populated on the rising edge of an airborne
    /// transition + read on landing for fall-damage calculation.
    fall_start_y: Option<f32>,
    /// Seconds remaining in the red screen-flash overlay that
    /// fires on any non-trivial damage event. Decays in update.
    hurt_flash: f32,
    /// Seconds elapsed since the last damage. Once it crosses
    /// `REGEN_DELAY`, HP starts ticking up at `REGEN_RATE` HP/s
    /// (capped at `PLAYER_MAX_HEALTH`).
    regen_accum: f32,
    /// Mobs the local player has killed this session. Bumped in
    /// `damage_mob` whenever the killing blow is attributed to the
    /// host (id 0). Surfaced in the HUD + F3 debug panel.
    kill_count: u32,
    /// Session counters for break / place actions the local
    /// player completed. Surfaced via /stats.
    blocks_broken: u64,
    blocks_placed: u64,
    /// Cumulative horizontal travel distance for /stats reports.
    /// Accumulates each tick whenever the player moves on the
    /// X/Z plane and resets at session boundaries (no persist).
    distance_walked: f32,
    /// Last frame's XZ position used for the delta calculation.
    last_walk_xz: Option<(f32, f32)>,
    /// Recent local placement history (oldest first). Drives the
    /// Z-key "undo last place" affordance. Capped to a small
    /// window so a long session doesn't bloat memory.
    place_history: Vec<[i32; 3]>,
    z_prev: bool,
    /// G toggles a wireframe overlay of the chunks immediately
    /// around the player. Useful for verifying streaming bounds
    /// + chunk loading patterns.
    show_chunk_grid: bool,
    g_prev: bool,
    /// Targeting wireframe mode. H cycles
    /// `Auto -> Build -> Break -> Auto`. Persisted per-player in
    /// `PlayerSave::highlight_mode` so the choice survives across
    /// sessions.
    highlight_mode: HighlightMode,
    /// Edge-detect helper for the H toggle: avoids re-triggering
    /// every frame the key is physically held.
    h_prev: bool,
    /// Current camera vertical FOV in radians. Smoothly lerps
    /// toward a wider angle while the player sprints, snaps back
    /// when they stop. Read by both the wgpu camera + the egui
    /// HUD's screen-projection so the two stay in sync.
    fov_y_rad: f32,
    /// `true` for the last frame's "are we actually sprinting"
    /// state - used by the FOV lerp.
    sprinting: bool,
    /// Breath remaining in seconds (0..=AIR_MAX_SEC). Drains
    /// while submerged in a `drains_air` liquid; refills 4x as
    /// fast on the surface. Hitting zero triggers drowning
    /// damage at `DROWN_DAMAGE_PER_SEC`.
    player_air: f32,
    /// Sprint stamina in 0..STAMINA_MAX. Drains while sprinting,
    /// regens when not. Reaching zero blocks the sprint speed
    /// bonus until it regens above the small reactivation
    /// threshold.
    stamina: f32,
    /// Per-remote-player interpolation buffer. We keep the
    /// previous + current `PlayerMsg` and their arrival instants
    /// so the render path can lerp between them at `now - delay`,
    /// turning a choppy pose stream (one update per net tick) into
    /// smooth motion regardless of the send rate. Both Server and
    /// Client roles write into this map - the host needs the same
    /// smoothing for the joined players it renders locally.
    remote_interp: HashMap<u32, RemoteInterp>,
    /// Local player's display name. Sent to the server as the
    /// `Hello` name (so the server's `client_name` lookup returns
    /// it), used as the chat sender tag for the host, and pushed
    /// to other clients via `vx.names`. Defaults to a random
    /// `Player####` if the user hasn't set one in settings.
    player_name: String,
    /// Latest known display name per player id. Filled from the
    /// `vx.names` roster the server broadcasts on join/leave. Used
    /// for nameplates above remote players and the Tab playerlist
    /// overlay. The local player's id always maps to
    /// `self.player_name`; remote ids come from the server.
    player_names: HashMap<u32, String>,
    /// True while Tab is held. Drives the player list overlay
    /// shown in the HUD.
    tab_held: bool,
    /// Floating "+N <block>" toasts shown on the right side of
    /// the HUD whenever the player picks something up. Entries
    /// fade out after a couple of seconds; consecutive picks of
    /// the same block collapse into a single counter.
    pickup_toasts: Vec<(std::time::Instant, BlockId, u8)>,
    /// Flat slot array. Indices `0..HOTBAR_SLOTS` are the hotbar
    /// row (visible at the bottom of the screen, scrollable via
    /// mouse wheel); indices `HOTBAR_SLOTS..INVENTORY_SLOTS` are
    /// the extra rows shown when the player opens the inventory
    /// via `E`. Each slot holds an optional `(BlockId, count)` -
    /// `count` saturates at `STACK_MAX`.
    inventory: [Option<(BlockId, u8)>; INVENTORY_SLOTS],
    selected_slot: usize,
    /// True when the inventory overlay is up. Pauses player input
    /// + shows the cursor.
    inventory_open: bool,
    /// In multiplayer roles, Esc opens an overlay menu *without*
    /// pausing the world simulation - other peers shouldn't see a
    /// frozen ghost just because someone hit Esc. This flag drives
    /// the overlay; `update_play` keeps ticking either way.
    mp_menu_open: bool,
    /// 2x2 crafting grid laid out row-major (idx = row*2 + col).
    /// Drained on a successful craft. Only relevant while the
    /// inventory overlay is open.
    craft_grid: [Option<(BlockId, u8)>; 4],
    /// Accumulated mouse-wheel ticks since the last tick consume; the
    /// sign drives slot selection.
    wheel_accum: f32,
    /// `true` when the player's feet sit on a solid voxel; resets
    /// every frame after collision resolution.
    grounded: bool,

    viewport: (u32, u32),
    fps_ema: f32,
    /// Accumulated in-world time (seconds). Wraps every
    /// `DAY_LENGTH_S`. Drives sun direction + sky color so the
    /// world has a visible day/night cycle.
    day_time: f32,

    /// Networking role + state. `Offline` runs the demo unchanged;
    /// `Server` / `Client` flip on the per-tick replication path.
    net: NetRole,
    /// Authoritative edit log on the server. Late-joining clients
    /// get the full history rebroadcast so their generated world
    /// catches up with everyone else's. Server-only; clients leave
    /// it empty.
    edit_history: Vec<EditMsg>,
    /// Client ids the server saw last tick. Used to detect joins
    /// for the catch-up broadcast.
    known_client_ids: HashSet<u32>,
    /// Last time the server's `edit_history` was flushed to disk.
    /// `None` until the first save fires (30 s after Play start);
    /// any later instant gates the next periodic flush.
    last_state_save: Option<std::time::Instant>,
    /// Side map from client name -> their persisted inventory.
    /// Populated on `load_server_state` for hosts; consulted when
    /// a client connects so reconnects pick up the same stacks.
    /// Empty in offline / client modes.
    saved_inventories: HashMap<String, ClientInv>,
    /// Slug of the world this session is bound to. Drives the
    /// `worlds/<slug>/` directory that `world_save::save_world` /
    /// `world_save::load_world` use. Defaulted to `Some("legacy")`
    /// for the cold-start path: the legacy-migration step in
    /// `load_server_state` overrides it if a pre-A4 single-file save
    /// is present, and the Worlds picker (Phase G scaffolding in B.7a)
    /// rewrites it on a New / Load click.
    current_world_slug: Option<String>,
    /// Monotonic tick counter incremented at the start of each
    /// `tick_net` call. Used to throttle pending-edit / pending-
    /// inventory-action resends so we don't spam UDP every frame.
    net_tick: u64,
    /// Audio mixer. Lazily initialised on first `on_start` because
    /// the cpal backend can fail (no output device, locked driver,
    /// etc.) and we don't want the demo to refuse to launch.
    audio: Option<infengine_audio::AudioSystem>,
    /// Multi-variant sound bank keyed by event name (e.g.
    /// `"dig:grass"`, `"step:wood"`, `"mob/sheep/say"`). Loaded
    /// from `--sounds <dir>` on startup; missing keys fall back
    /// to the procedural synth in `sfx_at`.
    audio_bank: infengine_audio::AudioBank,
    /// Optional directory of .ogg sound effects. When set, `on_start`
    /// tries to load `break.ogg`, `place.ogg`, `step.ogg` into
    /// `sound_assets`.
    sounds_dir: Option<std::path::PathBuf>,
    /// Footstep cooldown: counts down each frame while the player
    /// is moving + grounded. Resets to STEP_PERIOD whenever a
    /// step sting fires.
    step_cooldown: f32,
    /// Seconds since the last heartbeat thump. Drives the low-HP
    /// audio cue: fires more rapidly the closer the player is to
    /// dying. Stays at 0 above the threshold.
    heartbeat_accum: f32,
    /// Cactus contact damage interval timer.
    cactus_damage_accum: f32,
    /// Advancing phase used by the camera head-bob. Increments
    /// each frame proportionally to the player's horizontal speed
    /// when grounded; eases back toward zero off-ground. Kept
    /// separate from `day_time` so a stationary player has a
    /// stable view.
    head_bob_phase: f32,
    /// 0 = first person, 1 = third person behind, 2 = third
    /// person front. Cycled with F5 the way Minecraft does it.
    camera_mode: u8,
    f5_prev: bool,
    /// F6 toggles a runtime debug panel with sliders for time of
    /// day, weather, FOV, render distance, etc.
    show_debug_panel: bool,
    f6_prev: bool,
    /// Debug toggle for liquid spread bias-toward-drops
    /// pathfinding. When false, spread is purely radial (every
    /// air neighbour gets level+1 regardless of downhill
    /// distance). Useful for visualising the raw cellular
    /// automaton without the cliff-streaming heuristic.
    liquid_bias_drops: bool,
    /// F4 cycles weather modes; mirrors /weather. Host
    /// broadcasts the change so peers stay in sync.
    f4_prev: bool,
    q_prev: bool,
    /// Player's saved home position. /sethome stores the current
    /// pose; /home teleports back. Persisted via the config file
    /// so the location survives across launches.
    home_pos: Option<[f32; 3]>,
    /// Custom respawn point. None falls back to the origin
    /// pad. Persisted to the config file like home_pos.
    spawn_pos: Option<[f32; 3]>,
    /// Wallclock instant the session started running. Used by the
    /// F3 debug overlay to show how long the player has been
    /// playing. Stays None until on_start runs.
    session_started: Option<std::time::Instant>,
    /// Last time the server broadcast its authoritative
    /// `day_time`. Used to throttle the broadcast cadence to
    /// roughly once per second.
    last_day_broadcast: Option<std::time::Instant>,
    /// Creative-style fly mode. Toggled by `/fly`. Disables
    /// gravity + fall damage and remaps Space / Shift to vertical
    /// thrust. Off by default so a vanilla session is unchanged.
    fly_mode: bool,
    /// `/godmode` toggle. When on, apply_player_damage is a
    /// no-op so the local player can't be hurt. Useful for
    /// testing the world without dying.
    god_mode: bool,
    /// Brief post-respawn invincibility timer in seconds.
    /// `apply_player_damage` swallows hits while > 0 so the
    /// player can recover after a death drop without dying again
    /// instantly to a lingering mob.
    invuln_remaining: f32,
    /// Tracks the previous tick's is_night state so the host can
    /// emit chat lines on the day <-> night transitions without
    /// spamming every frame.
    was_night: bool,
    /// Tracks the previous tick's submerged state so we can
    /// fire a splash sting + droplet particles on the air ->
    /// water transition (and a quieter exit cue going out).
    was_in_water: bool,
    /// `/lock` toggle on the host. When on, client-issued
    /// `vx.edit` packets are dropped so only the host can modify
    /// the world. Useful for showcase/no-grief sessions.
    world_locked: bool,
    /// Maximum concurrent live mob count. Tweakable via /mobcap
    /// so players on slow machines can cut the spawn pressure;
    /// defaults to `MOB_CAP_DEFAULT`.
    mob_cap: usize,
    /// `/freezetime` toggle. When on, the day_time clock stops
    /// advancing so the sun + sky stay frozen. Useful for
    /// screenshots / debugging.
    freeze_time: bool,
    /// 0 = clear, 1 = rain. Drives per-frame rain particle
    /// spawning above the player so the demo can have weather
    /// without actually wiring it into the gameplay.
    weather: u8,
    /// Names the player has muted via /mute. Chat lines whose
    /// sender tag matches one of these (case-insensitive) are
    /// dropped before they hit `chat_log`. Stored by lowercased
    /// name to keep matching trivial.
    muted_players: HashSet<String>,
    /// Seconds remaining in the current lightning flash. While
    /// positive the sky washes toward white. Decays each frame
    /// via the update tick.
    lightning_flash: f32,
    /// Accumulator that fires periodic snow-layer placement
    /// while weather=2 (host-only). Drives the visual snow
    /// blanket that grows on top of nearby grass voxels.
    snow_accum: f32,
    /// Seconds between snow paint events while weather=2. Lower
    /// = denser snowfall. Adjustable from the F6 debug panel.
    snow_rate: f32,
    /// Accumulator for the optional chicken feather-laying debug
    /// loop. While `debug_chicken_feathers` is on, hosts drop one
    /// feather into the host's inventory every ~12 s as long as a
    /// live chicken exists. Off by default - see the Q.3 fix
    /// (feathers were spawning continuously in inventory mid-air
    /// before this got gated). Toggle via `/debug feathers on|off`.
    chicken_lay_accum: f32,
    /// Real-time accumulator that turns the variable-dt frame
    /// loop into 20 Hz logical drop ticks. Drained inside
    /// `drops::tick_physics` - every `TICK_DT_SECS` (0.05 s) of
    /// real time it fires one `drops::step_logical_tick`. Lives
    /// here because the loop may straddle multiple ticks during a
    /// slow frame and the accumulator has to persist between
    /// frames.
    drop_tick_accum: f32,
    /// Gate for the periodic chicken feather lay loop. Defaults to
    /// `false`; flip on with `/debug feathers on` for testing.
    debug_chicken_feathers: bool,
    /// Seconds until the next lightning strike attempt while
    /// weather=1. Initialised to a positive value so the first
    /// strike isn't instant on /weather rain.
    next_lightning_at: f32,
    /// F3 toggle: show a debug-info panel with player pos, chunk
    /// counts, net status, etc. Default off so the casual HUD
    /// stays clean.
    show_debug: bool,
    f3_prev: bool,
    /// World coords of every placed torch. Updated on every world
    /// edit (break clears, place inserts). Used per frame to push
    /// point lights to the renderer near the player.
    torch_positions: HashSet<[i32; 3]>,
    /// Sticky torch orientations keyed by voxel. 0 = floor,
    /// 1 = wall pointing -X (mounted on +X face of a block at
    /// voxel.x+1), 2 = +X, 3 = -Z, 4 = +Z. Set at placement
    /// time from the raycast normal so neighbouring builds
    /// don't reorient an existing torch.
    torch_orient: HashMap<[i32; 3], u8>,
    /// Voxels currently holding a thin snow layer + the layer
    /// count (1..=8). MC-style stacking: each fall raises an
    /// existing voxel's layer until it tops out, then a fresh
    /// snow voxel is painted on top.
    snow_positions: HashMap<[i32; 3], u8>,
    /// Per-chest container state, keyed by absolute world voxel
    /// coords. An entry is inserted when the player places a
    /// chest block + removed when the chest is broken (after the
    /// contents drop as item entities). The container itself is
    /// a 27-slot `Container` (see `block::chest::CHEST_SLOTS`).
    /// Persisted via `world_save/containers.rs`.
    chests: HashMap<[i32; 3], container::Container>,
    /// Position of the chest the player has open. `None` means no
    /// chest UI is visible. Set by an RMB-on-chest interaction +
    /// cleared by ESC / walking too far / opening another panel.
    chest_open: Option<[i32; 3]>,
    /// Cursor-held stack while the chest UI is open. Mirrors how
    /// the standard inventory UI tracks a half / full stack drag;
    /// here it's a simple `Option<(item_id, count)>` because the
    /// chest panel uses the generic `container::ui::draw_grid`
    /// click-and-drop flow rather than the legacy DnD widget.
    chest_held: Option<(u16, u8)>,
    /// Per-furnace 3-slot container state (input / fuel / output),
    /// keyed by absolute world voxel coords. Parallel to `chests`.
    /// Entries are inserted on place + removed on break (after the
    /// drops fall as item entities). The runtime cook progress
    /// lives in the sibling `furnace_runtimes` map below; splitting
    /// the slot vec from the live timers keeps the save schema
    /// tight (the on-disk shape persists both side by side under
    /// `containers/<dim>.bin` v2).
    furnaces: HashMap<[i32; 3], container::Container>,
    /// Per-cell fire age (0..=15), keyed by absolute world voxel
    /// coords. Q.4b: parallel to `chests` / `furnaces`. The block
    /// at the keyed cell is `FIRE_ID`; the age value drives the
    /// extinguish + spread random-tick algorithm (MC
    /// FireBlock.java::tick).
    fires: HashMap<[i32; 3], u8>,
    /// Per-furnace live runtime: remaining fuel ms + cook progress
    /// ms + the full fuel duration for the UI bar. Driven by
    /// `block::furnace::furnace_tick`; persisted alongside the
    /// slot vec.
    furnace_runtimes: HashMap<[i32; 3], block::furnace::FurnaceRuntime>,
    /// Position of the furnace the player has open. Mirrors
    /// `chest_open`. Cleared by ESC / walking too far / opening
    /// another panel.
    furnace_open: Option<[i32; 3]>,
    /// Cursor-held stack while the furnace UI is open. Mirrors
    /// `chest_held`.
    furnace_held: Option<(u16, u8)>,
    /// MC-style per-cell tick schedule. `world_set_tracked` and
    /// the tick loop queue cells here with a `due_at_tick`; each
    /// frame pops due entries up to `MAX_LIQUID_PER_FRAME`. No
    /// hard cap on the schedule itself - cells that don't fit a
    /// frame keep their slot and tick on the next.
    ///
    /// Replaces the old `liquid_active: HashSet` sweep: the
    /// schedule's `pending` set serves the same "is this cell
    /// scheduled?" role.
    liquid_schedule: LiquidSchedule,
    /// Game-tick counter for the schedule. +1 per net-tick frame.
    /// Used both as `now` for `pop_due` and as the base for
    /// computing `due_at_tick = now + tick_delay`.
    liquid_clock: u64,
    /// Every liquid voxel currently in the world. Maintained on
    /// every peer (host + clients) so the dedicated slope-mesh
    /// render path can iterate without scanning chunks. The
    /// block id at the world coord tells the mesher which
    /// liquid lives at that cell.
    liquid_positions: HashSet<[i32; 3]>,
    /// MC-style level per liquid voxel: 0 = source, 1..=7 = flow
    /// (lower = closer to source). Missing entry defaults to 0
    /// (source) so loaders from old saves treat existing fluid
    /// as a source.
    liquid_level: HashMap<[i32; 3], u8>,
    /// Server-authoritative mob list (sheep + future kinds). Only
    /// populated in `NetRole::Server` / `NetRole::Offline`. Each
    /// tick the server updates pos / yaw + broadcasts `vx.mob`.
    mobs: Vec<MobState>,
    /// Client-side view of remote mobs: `id -> latest MobMsg`.
    /// In offline / server mode this stays in sync with `mobs` so
    /// the same render path works.
    remote_mobs: HashMap<u32, MobMsg>,
    /// Day-clock value of the last successful mob spawn. Used to
    /// throttle spawn attempts to one every `MOB_SPAWN_PERIOD`
    /// seconds.
    last_mob_spawn_time: f32,
    /// Counter for the next mob id. Reset to 1 because 0 is the
    /// host player's id in the pose table - no point in collisions.
    next_mob_id: u32,
    /// Per-mob step-sound cooldown (seconds). Drained while the
    /// mob is moving; sound fires + resets when it reaches zero.
    mob_step_timers: HashMap<u32, f32>,
    /// Per-mob baa/say cooldown (seconds). Decrements every tick;
    /// fire + roll a new random delay when it reaches zero.
    mob_say_timers: HashMap<u32, f32>,
    /// Last known (x, z) for each mob - used to detect movement
    /// for step-sound triggering on the client (the server doesn't
    /// broadcast velocity, so we infer from pos deltas).
    mob_last_xz: HashMap<u32, [f32; 2]>,
    /// Visual hurt-flash decay per mob (seconds remaining). Tinted
    /// red while > 0, fades each frame. Populated locally on the
    /// attacker + by `vx.mob_event` broadcasts on every other
    /// peer so everyone sees the flash.
    mob_hurt_flash: HashMap<u32, f32>,
    /// Seconds since each mob last took damage. Drives HP regen
    /// so mobs only heal when out of combat. Reset on every
    /// `damage_mob` hit, advanced each tick by `update_mobs`.
    mob_no_hit_time: HashMap<u32, f32>,
    /// Seconds remaining in the local death-fade animation per
    /// mob. Server populates from `MobState.dying` when a kill
    /// fires; client populates on `vx.mob_event { kind: 1 }`. The
    /// render path tilts + fades the cube while the timer counts
    /// down to zero.
    mob_dying: HashMap<u32, f32>,
    /// Per-mob lunge-animation timer. Set to `MOB_LUNGE_TIME` on
    /// every melee swing (currently only zombies). Renderer reads
    /// it to push the body briefly forward + return.
    mob_lunge: HashMap<u32, f32>,
    /// Cube mesh used to render every remote player as a coloured
    /// 1x1x1 marker. Built lazily once the renderer is up.
    player_cube: Option<MeshHandle>,
    /// Slope-aware mesh per liquid kind (one handle for water,
    /// one for lava, ...). The renderer draws each with its own
    /// PBR colour so different liquids can share the same
    /// geometry pipeline. Rebuilt lazily when
    /// `liquid_mesh_dirty` flips (place / fall / spread / decay
    /// / source merge).
    liquid_mesh_handles: HashMap<u16, MeshHandle>,
    liquid_mesh_dirty: bool,
    /// Worldgen variant the current chunks were generated from.
    /// Compared against `WorldGen::current()` on `Play offline`
    /// so the world rebuilds when the user picks a different
    /// variant in the main-menu combo. `None` until first
    /// `build_world` runs.
    last_built_worldgen: Option<WorldGen>,
    /// Floating drop entities the world (host-authoritative).
    /// Each tick the host steps physics, runs pickup, broadcasts
    /// spawn / pickup messages. Clients keep a mirrored Vec
    /// purely for rendering.
    dropped_items: Vec<DropItem>,
    /// Monotonic id source for new drops on the host. Clients
    /// keep at 0; ids come from the server.
    next_drop_id: u32,
    /// Cooldown timer for the running-water ambient SFX. Ticks
    /// down each frame the player stands in a flow voxel; the
    /// running-stream sting fires when it hits zero and resets.
    flow_sfx_accum: f32,
    /// Dedicated cube mesh for sheep bodies. Has its own
    /// `AlbedoHandle` binding so the texture (loaded from
    /// `assets/textures/entity/sheep/sheep.tga`) doesn't leak onto
    /// the player marker cubes.
    sheep_mesh: Option<MeshHandle>,
    /// Dedicated cube mesh for sheep heads, bound to a 16x16
    /// crop of the MC sheep entity texture (the face region).
    /// Falls back to flat-colour rendering on `player_cube` when
    /// the source `sheep.tga` is missing.
    sheep_head_mesh: Option<MeshHandle>,
    /// CPU particle system: short-lived debris on every block break.
    particles: infengine_particles::ParticleSystem,
    /// Shared atlas bind group for the voxel chunk submeshes. Built
    /// once in `on_start`; every new chunk submesh clones the Arc
    /// instead of uploading a fresh texture.
    voxel_atlas: Option<infengine_render::AlbedoHandle>,
    /// Raw RGBA copy of the block atlas, retained so the egui UI
    /// can upload its own texture handle for slot icons (without
    /// re-reading the disk / re-running the procedural builder).
    atlas_rgba: Option<(u32, u32, Vec<u8>)>,
    /// egui texture handle for the block atlas. Loaded lazily on
    /// first inventory render so we don't pay the upload cost until
    /// the player actually opens the menu.
    egui_atlas: Option<egui::TextureHandle>,
    /// Optional directory of PNG / TGA block textures. When set,
    /// `on_start` tries to load the atlas from there before
    /// falling back to the procedural builder.
    texture_dir: Option<std::path::PathBuf>,
    /// Master-server endpoint the in-game browser queries. Settable
    /// via `--master <addr>` argv; defaults to the configured
    /// public master.
    master_addr: String,
    /// Buffers + state backing the three new menu screens. Lifted
    /// out of `AppState` so the enum stays Copy.
    direct_addr_buf: String,
    host_name_buf: String,
    host_port_buf: String,
    host_announce: bool,
    browser_servers: Vec<infengine_net::master::ServerInfo>,
    browser_error: Option<String>,
    browser_last_refresh: Option<std::time::Instant>,
    /// In-flight master `ListServers` query. The refresh path now
    /// runs on a worker thread so the UI doesn't freeze for the
    /// 2-second master timeout. `Some` while a query is pending;
    /// cleared on result delivery.
    browser_query_rx:
        Option<std::sync::mpsc::Receiver<Result<Vec<infengine_net::master::ServerInfo>, String>>>,
    /// In-flight master `Register` query (host announce). Same
    /// async pattern as the browser refresh.
    announce_query_rx: Option<std::sync::mpsc::Receiver<Result<u64, String>>>,
    /// Returned by the master server on successful Register; the
    /// host echoes it back on Heartbeat + Unregister.
    master_token: Option<u64>,
    /// Last heartbeat we sent to the master (server mode). Drives
    /// the periodic re-heartbeat in `tick_net`.
    last_heartbeat: Option<std::time::Instant>,
    /// Buffer for the "New world" name input in the main-menu Worlds
    /// picker (Phase B.7a scaffolding). The user types a display name
    /// here; `layout::slug()` canonicalises it before
    /// `current_world_slug` is set.
    new_world_name_buf: String,
}

impl Voxel {
    fn new_with_net(net: NetRole) -> Self {
        let mut v = Self::new();
        v.net = net;
        v
    }

    /// After a block is broken at `removed`, scan the 5 attaching
    /// faces (above + NSEW) and pop off any `F_NO_PLACE`
    /// decoration that no longer has support. Each dislodged
    /// decoration spawns as a drop entity (pickup), broadcasts a
    /// `vx.edit` packet, and re-enters the queue so cascades
    /// (torch on torch on dirt) all unwind in one call.
    fn dislodge_unsupported_decorations(&mut self, removed: [i32; 3]) {
        decorations::dislodge_unsupported(self, removed);
    }

    /// Set a single voxel in the world + maintain the torch index
    /// so dynamic point lights track placed/broken torches. Every
    /// edit-apply path funnels through here so we only have one
    /// place that knows about torch bookkeeping.
    fn world_set_tracked(&mut self, voxel: [i32; 3], block: BlockId) {
        self.world.set(voxel, block);
        // Both torches + glowstone act as point-light emitters.
        if block.0 == TORCH_ID || block.0 == GLOWSTONE_ID {
            self.torch_positions.insert(voxel);
        } else {
            self.torch_positions.remove(&voxel);
        }
        // Snow layer voxels render as a thin slab in the world
        // pass; tracked separately so the renderer can skip
        // the full-cube path. Inserts default to 1 layer; the
        // accumulator path bumps the value when more snow lands
        // on the same voxel.
        if block.0 == 15 {
            self.snow_positions.entry(voxel).or_insert(1);
        } else {
            self.snow_positions.remove(&voxel);
        }
        // Torch removal also clears the sticky orientation
        // override so a future placement starts fresh.
        if block.0 != TORCH_ID {
            self.torch_orient.remove(&voxel);
        }
        // Liquid bookkeeping: any registered liquid (water,
        // lava, ...) re-enters the active queue + position set
        // when it's written. Level defaults to 0 (source) for
        // direct writes; tick passes overwrite when they
        // propagate flow from a source. Non-liquid writes purge
        // the entry so a broken liquid cell doesn't leak into
        // the tracker.
        if let Some(liquid) = liquid_for(block) {
            // MC-style: the freshly placed cell ticks after
            // `tick_period` frames - matches the legacy cadence
            // the planner enforced via `nt % tick_period`.
            let when = self.liquid_clock + liquid.tick_period.max(1) as u64;
            self.liquid_schedule.schedule(voxel, when);
            self.liquid_level.entry(voxel).or_insert(0);
            if self.liquid_positions.insert(voxel) {
                self.liquid_mesh_dirty = true;
            }
        } else if self.liquid_positions.remove(&voxel) {
            self.liquid_level.remove(&voxel);
            // The cell is no longer liquid - drop its slot in
            // the schedule. The matching heap entry becomes a
            // tombstone, popped + skipped on the next sweep.
            self.liquid_schedule.cancel(voxel);
            self.liquid_mesh_dirty = true;
        } else {
            self.liquid_level.remove(&voxel);
        }
        // Any world edit can change a neighbour's air / solid /
        // water status, which feeds into both the corner-height
        // blend and the liquid simulation - a settled pool cell
        // needs to wake up the moment the player digs out a hole
        // anywhere within flow range.
        //
        // Scan a 3x3x3 cube around the touched voxel and pull
        // adjacent liquid cells back into the schedule so
        // the next tick re-evaluates their flow. The wider 5x5x5
        // sweep was overhead-heavy on big pools; the tick's
        // own NSEW propagation handles cells past the 3x3 ring.
        let mut neighbour_liquid = false;
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if dx == 0 && dy == 0 && dz == 0 {
                        continue;
                    }
                    let n = [
                        voxel[0] + dx,
                        voxel[1] + dy,
                        voxel[2] + dz,
                    ];
                    if self.liquid_positions.contains(&n) {
                        neighbour_liquid = true;
                        // MC-style: re-schedule each adjacent
                        // liquid cell at a short delay so the
                        // spread can react to the new topology
                        // (block placed in front of a stream,
                        // pool floor dug out). MC uses
                        // 1..tickDelay; we use a 1-tick wake so
                        // the response shows up next frame.
                        self.liquid_schedule
                            .schedule(n, self.liquid_clock + 1);
                    }
                }
            }
        }
        if neighbour_liquid {
            self.liquid_mesh_dirty = true;
            // The 1-tick neighbour wake above already gets us
            // the next-frame response - no separate force flag
            // needed in the per-cell schedule model.
        }
    }

    /// Thin wrapper - see `mob_ai::damage_mob`.
    fn damage_mob(&mut self, mob_id: u32, attacker_client_id: Option<u32>) {
        mob_ai::damage_mob(self, mob_id, attacker_client_id);
    }

    /// Thin wrapper - see `mob_ai::damage_mob_scaled`.
    fn damage_mob_scaled(
        &mut self,
        mob_id: u32,
        attacker_client_id: Option<u32>,
        mult: f32,
    ) {
        mob_ai::damage_mob_scaled(self, mob_id, attacker_client_id, mult);
    }

    /// Thin wrapper - see `mob_ai::raycast_mob`.
    fn raycast_mob(
        &self,
        origin: Vec3,
        dir: Vec3,
        max_dist: f32,
    ) -> Option<(u32, f32)> {
        mob_ai::raycast_mob(self, origin, dir, max_dist)
    }

    /// Find the world-space Y of the topmost solid voxel at column
    /// `(wx, wz)` at or below `max_y`. `None` if the column is air
    /// all the way down.
    ///
    /// We scan from `max_y` downward (instead of from the world
    /// ceiling) so a sheep walking under a tree's leaf canopy
    /// doesn't get its surface snap teleported up to the canopy
    /// top - the relevant ground is the first solid block below
    /// the mob's current head, not the highest block in the
    /// column.
    fn surface_y_below(&self, wx: i32, wz: i32, max_y: i32) -> Option<i32> {
        let top = max_y.min((CHUNK_SIZE as i32) * VERTICAL_CHUNKS - 1).max(0);
        (0..=top).rev().find(|&y| !self.world.get([wx, y, wz]).is_air())
    }

    /// Resolve a safe player spawn for the current world. Uses
    /// `spawn_pos` when configured, otherwise scans an outward
    /// spiral around the origin for a column whose top voxel is
    /// solid + has clear air above. Falls back to high-altitude
    /// Y=90 so gravity can drop the player onto whatever terrain
    /// exists below. Caller should clear `player_vel` +
    /// `fall_start_y` after assigning the returned position.
    fn safe_spawn(&self) -> Vec3 {
        if let Some(p) = self.spawn_pos {
            return Vec3::new(p[0], p[1], p[2]);
        }
        let ceiling = (CHUNK_SIZE as i32) * VERTICAL_CHUNKS - 1;
        // Square-spiral search out to 24 blocks. The first column
        // with a confirmed surface + 2 blocks of air directly
        // above wins so the player doesn't ever spawn buried.
        for r in 0i32..=24 {
            for dz in -r..=r {
                for dx in -r..=r {
                    // Cheap ring filter: only inspect cells on
                    // the current ring perimeter.
                    if r > 0 && dx.abs() != r && dz.abs() != r {
                        continue;
                    }
                    let sy = match self.surface_y_below(dx, dz, ceiling) {
                        Some(y) => y,
                        None => continue,
                    };
                    // Need 3 clear air blocks above the surface
                    // voxel so the player capsule (PLAYER_HEIGHT
                    // = 1.7 m) has headroom + body room.
                    let above1 = self.world.get([dx, sy + 1, dz]);
                    let above2 = self.world.get([dx, sy + 2, dz]);
                    let above3 = self.world.get([dx, sy + 3, dz]);
                    if above1.is_air() && above2.is_air() && above3.is_air() {
                        // Voxel `sy` is solid with top at world
                        // y = sy + 1. Foot sits on that surface;
                        // `player_pos.y` is the eye/cap height so
                        // it lands at sy + 1 + PLAYER_HEIGHT.
                        return Vec3::new(
                            dx as f32 + 0.5,
                            sy as f32 + 1.0 + PLAYER_HEIGHT + 0.05,
                            dz as f32 + 0.5,
                        );
                    }
                }
            }
        }
        Vec3::new(0.0, 90.0, 0.0)
    }

    /// Height of a single corner of voxel (vx,vy,vz). `cx`/`cz`
    /// in `{0,1}` pick which corner. Two-phase computation:
    ///
    ///   Phase 1: if any of the four touching cells is a falling
    ///   column (same liquid directly above), the corner snaps
    ///   to 1.0 - submerged / column voxels render as full
    ///   blocks, no slope.
    ///
    ///   Phase 2 (top-layer surface): average the surface
    ///   heights of touching cells, counting AIR as 0 height
    ///   and SKIPPING solid blocks entirely. This gives the
    ///   top of a pool a sloped lip toward the bank instead of
    ///   the boxy vanilla MC look. Solid neighbours are
    ///   ignored so a pond against a stone wall keeps the
    ///   wall-side corner at the source height.
    /// Horizontal flow direction at a liquid voxel, returned as
    /// a unit-ish 2D vector in (x, z). Computed by summing the
    /// "flow toward lower level" contribution of each NSEW
    /// neighbour - the cell pushes downhill toward lower-level
    /// flow cells, off cliffs (cell with air below), and out of
    /// itself. Source cells with no lower-level neighbour return
    /// the zero vector (no current).
    fn liquid_flow_dir(&self, pos: [i32; 3], liquid_id: u16) -> [f32; 2] {
        let level = self.liquid_level.get(&pos).copied().unwrap_or(0);
        let mut dx = 0.0_f32;
        let mut dz = 0.0_f32;
        let dirs: [([i32; 3], [f32; 2]); 4] = [
            ([1, 0, 0], [1.0, 0.0]),
            ([-1, 0, 0], [-1.0, 0.0]),
            ([0, 0, 1], [0.0, 1.0]),
            ([0, 0, -1], [0.0, -1.0]),
        ];
        for (d, axis) in &dirs {
            let n = [pos[0] + d[0], pos[1] + d[1], pos[2] + d[2]];
            let nb = self.world.get(n);
            if nb.0 == liquid_id {
                let nl = self.liquid_level.get(&n).copied().unwrap_or(0);
                if nl > level {
                    // Neighbour lower in the surface ramp -> our
                    // flow vectors toward it.
                    let weight = (nl - level) as f32;
                    dx += axis[0] * weight;
                    dz += axis[1] * weight;
                }
            } else if nb.is_air() {
                // Air neighbour with air below = cliff edge.
                // Flow points outward.
                let below_n = [n[0], n[1] - 1, n[2]];
                if self.world.get(below_n).is_air() {
                    dx += axis[0];
                    dz += axis[1];
                }
            }
        }
        let mag = (dx * dx + dz * dz).sqrt();
        if mag < 1e-3 {
            [0.0, 0.0]
        } else {
            [dx / mag, dz / mag]
        }
    }

    fn liquid_corner_height(
        &self,
        vx: i32,
        vy: i32,
        vz: i32,
        cx: i32,
        cz: i32,
        liquid_id: u16,
    ) -> f32 {
        liquid_corner_height_at(
            &self.world,
            &self.liquid_level,
            vx,
            vy,
            vz,
            cx,
            cz,
            liquid_id,
        )
    }

    /// Rebuild the combined water mesh when the dirty flag is
    /// set. One handle covers every visible water voxel: top
    /// face uses per-corner heights computed from the depth map,
    /// side faces drop to y=0 only against air / decorations, and
    /// the bottom face emits only when there's something the
    /// player could actually see through the water from below.
    fn maybe_rebuild_liquid_mesh(&mut self) {
        if !self.liquid_mesh_dirty {
            return;
        }
        if self.renderer.is_none() {
            return;
        }
        // Per-liquid vertex/index pool. One mesh per kind so
        // each can draw with its own colour (water blue, lava
        // orange, ...) through the shared alpha-blend pipeline.
        let mut buckets: HashMap<u16, (Vec<MeshVertex>, Vec<u32>)> =
            HashMap::new();
        for p in self.liquid_positions.iter().copied() {
            let block = self.world.get(p);
            let liquid_id = block.0;
            if liquid_for(block).is_none() {
                continue;
            }
            let (vertices, indices) =
                buckets.entry(liquid_id).or_default();
            let vx = p[0];
            let vy = p[1];
            let vz = p[2];
            let h00 = self.liquid_corner_height(vx, vy, vz, 0, 0, liquid_id);
            let h10 = self.liquid_corner_height(vx, vy, vz, 1, 0, liquid_id);
            let h11 = self.liquid_corner_height(vx, vy, vz, 1, 1, liquid_id);
            let h01 = self.liquid_corner_height(vx, vy, vz, 0, 1, liquid_id);
            let fx = vx as f32;
            let fy = vy as f32;
            let fz = vz as f32;
            // Flow vector for this cell (units of voxels/sec on
            // the XZ plane). Baked into the top/side faces so the
            // animated water shader scrolls its caustic noise in
            // the right direction. Sources / still ponds return
            // zero and the shader falls back to an isotropic
            // shimmer.
            let flow = self.liquid_flow_dir([vx, vy, vz], liquid_id);
            // Top face: emit only when the voxel above isn't
            // the same liquid (i.e. we're not buried inside a
            // falling column).
            let above_same =
                self.world.get([vx, vy + 1, vz]).0 == liquid_id;
            if !above_same {
                let t0 = [fx, fy + h00, fz];
                let t1 = [fx + 1.0, fy + h10, fz];
                let t2 = [fx + 1.0, fy + h11, fz + 1.0];
                let t3 = [fx, fy + h01, fz + 1.0];
                let normal = water_face_normal(t0, t1, t2, t3);
                push_quad_flow(
                    vertices, indices, t0, t1, t2, t3, normal, flow,
                );
            }
            // Side faces: emit only when the in-plane neighbour is
            // air or a passable decoration. Same-liquid cancels -
            // shared face sits inside the body. See `side_open`
            // in `liquid::mesh` for the predicate definition.
            if side_open(&self.world, vx - 1, vy, vz, liquid_id) {
                push_quad_flow(
                    vertices, indices,
                    [fx, fy, fz + 1.0],
                    [fx, fy + h01, fz + 1.0],
                    [fx, fy + h00, fz],
                    [fx, fy, fz],
                    [-1.0, 0.0, 0.0],
                    flow,
                );
            }
            if side_open(&self.world, vx + 1, vy, vz, liquid_id) {
                push_quad_flow(
                    vertices, indices,
                    [fx + 1.0, fy, fz],
                    [fx + 1.0, fy + h10, fz],
                    [fx + 1.0, fy + h11, fz + 1.0],
                    [fx + 1.0, fy, fz + 1.0],
                    [1.0, 0.0, 0.0],
                    flow,
                );
            }
            if side_open(&self.world, vx, vy, vz - 1, liquid_id) {
                push_quad_flow(
                    vertices, indices,
                    [fx, fy, fz],
                    [fx, fy + h00, fz],
                    [fx + 1.0, fy + h10, fz],
                    [fx + 1.0, fy, fz],
                    [0.0, 0.0, -1.0],
                    flow,
                );
            }
            if side_open(&self.world, vx, vy, vz + 1, liquid_id) {
                push_quad_flow(
                    vertices, indices,
                    [fx + 1.0, fy, fz + 1.0],
                    [fx + 1.0, fy + h11, fz + 1.0],
                    [fx, fy + h01, fz + 1.0],
                    [fx, fy, fz + 1.0],
                    [0.0, 0.0, 1.0],
                    flow,
                );
            }
            // Bottom face: emit when the cell below is open (air or
            // passable decoration). Skipping this when below is air
            // was a bug - looking up from underneath a cliff-edge
            // water cell showed the air gap as plain sky instead of
            // the water bottom. MC's `LiquidBlockRenderer` renders
            // a bottom face in the same case via its
            // `shouldRenderFace(DOWN)` gate.
            //
            // Same-liquid below => no face (would be inside the
            // body). Solid below => no face (covered by the solid's
            // own top face).
            if bottom_open(&self.world, vx, vy - 1, vz, liquid_id) {
                push_quad_flow(
                    vertices, indices,
                    [fx, fy, fz],
                    [fx + 1.0, fy, fz],
                    [fx + 1.0, fy, fz + 1.0],
                    [fx, fy, fz + 1.0],
                    [0.0, -1.0, 0.0],
                    flow,
                );
            }
        }
        // Re-borrow renderer now that the `&self.world` /
        // `&self.liquid_level` reads above are done.
        let renderer = self
            .renderer
            .as_mut()
            .expect("renderer checked at function entry");
        // Drop every previous handle: the rebuild owns the
        // full state so we don't try to diff per-cell.
        for (_, h) in self.liquid_mesh_handles.drain() {
            renderer.remove_mesh(h);
        }
        for (liquid_id, (vertices, indices)) in buckets {
            if vertices.is_empty() || indices.is_empty() {
                continue;
            }
            let label = format!("voxel_liquid_{liquid_id}");
            let h = renderer.create_mesh(&label, &vertices, &indices);
            if block_is_transparent_render(BlockId(liquid_id)) {
                renderer.set_mesh_transparent(h, true);
            }
            self.liquid_mesh_handles.insert(liquid_id, h);
        }
        self.liquid_mesh_dirty = false;
    }

    /// Spawn a fresh drop entity at the voxel centre. Host-only
    /// path: assigns a new id, appends to `dropped_items`, and
    /// broadcasts a `vx.drop_spawn` so every connected peer
    /// renders it. Caller passes the source `block` (whatever
    /// was just broken or washed) - air drops are skipped.
    fn spawn_drop(&mut self, block: BlockId, voxel: [i32; 3]) {
        drops::spawn(self, block, voxel);
    }

    /// Step every drop's physics + pickup logic for one frame.
    /// Host runs the full simulation. Clients only advance the
    /// age + apply a simple gravity preview so the bob looks
    /// right between server snapshots; pickup is host-side.
    fn tick_drops(&mut self, dt: f32) {
        drops::tick_physics(self, dt);
    }

    /// Per-frame scheduler. Walks the `LIQUIDS` registry and
    /// dispatches each liquid's tick passes when the cadence
    /// (`Liquid::tick_period`) matches. Host-only - clients
    /// receive the resulting `vx.edit` traffic.
    fn tick_liquids(&mut self) {
        if matches!(self.net, NetRole::Client { .. }) {
            return;
        }
        // Advance the per-cell schedule clock once per frame.
        self.liquid_clock = self.liquid_clock.wrapping_add(1);
        let now = self.liquid_clock;
        // Per-frame CPU budget for liquid cell processing. Cells
        // that don't fit this frame keep their slot in the
        // schedule and tick next frame - replaces the old hard
        // 8192 cap on the bulk planner. 4096 leaves plenty of
        // CPU headroom (4096 * ~20 world.get calls = ~80k reads
        // per frame, still well under 1 ms on a warm cache) and
        // lets a wave of 5-10k pending cells drain in 2-3 frames.
        const MAX_LIQUID_PER_FRAME: usize = 4096;
        let mut processed = 0usize;
        while processed < MAX_LIQUID_PER_FRAME {
            let Some(pos) = self.liquid_schedule.pop_due(now) else {
                break;
            };
            // Identify which liquid owns this cell, if any. The
            // cell may have been broken since it was scheduled -
            // skip silently.
            let b = self.world.get(pos);
            let Some(liquid) = liquid_for(b) else {
                continue;
            };
            // Snapshot the liquid by value (no &self borrow into
            // the inner method).
            let lq = *liquid;
            self.tick_liquid_cell(pos, &lq, now);
            processed += 1;
        }
        // Source-merge pass: each `infinite_source` liquid gets a
        // chance to promote flow cells. The merge is rare + fast
        // (256-cell cap inside `liquid_merge_step`); run it every
        // frame regardless of schedule state.
        for liquid in LIQUIDS {
            if liquid.infinite_source {
                self.tick_liquid_merge(liquid);
            }
        }
    }

    /// MC `FlowingFluid.tick(pos)`: pulls `pos` through
    /// `getNewLiquid`, applies + spreads, then re-schedules every
    /// neighbour the apply touched (and `pos` itself if still
    /// active) at `now + tick_period`.
    fn tick_liquid_cell(&mut self, pos: [i32; 3], liquid: &Liquid, now: u64) {
        let liquid_id = liquid.block_id;
        let delay = liquid.tick_period.max(1) as u64;
        // Build the per-cell plan against the current world snapshot.
        let mut plan = LiquidPlanStep::default();
        let mut updates: std::collections::HashMap<[i32; 3], (u8, bool)> =
            std::collections::HashMap::new();
        liquid_plan_cell(
            &self.world,
            &self.liquid_level,
            pos,
            liquid,
            self.liquid_bias_drops,
            &mut plan,
            &mut updates,
        );
        plan.place = updates.into_iter().map(|(p, (l, f))| (p, l, f)).collect();
        if plan.evaporate.is_empty() && plan.decay.is_empty() && plan.place.is_empty() {
            // Cell is settled. Don't reschedule - it'll only fire
            // again when a neighbouring edit/spread queues it.
            return;
        }
        // The schedule replaces the legacy `liquid_active` set;
        // pass a throwaway HashSet so `liquid_apply_plan` can
        // still maintain its internal wake-up shape. The
        // schedule itself catches the same wake via the explicit
        // re-schedule loop below.
        let mut scratch_active: HashSet<[i32; 3]> = HashSet::new();
        let tape = liquid_apply_plan(
            &mut self.world,
            &mut self.liquid_level,
            &mut scratch_active,
            &mut self.liquid_positions,
            liquid,
            plan,
        );
        // Drops + registry cleanup for washable replacements.
        for (block, p) in &tape.washed {
            if block.0 == 15 {
                self.snow_positions.remove(p);
            }
            self.spawn_drop(*block, *p);
        }
        // Rebuild every chunk the apply touched.
        let mut chunk_set: HashSet<ChunkCoord> = HashSet::new();
        for p in &tape.evaporated {
            for c in Self::affected_chunks(*p) {
                chunk_set.insert(c);
            }
        }
        for (p, _, _) in &tape.placed {
            for c in Self::affected_chunks(*p) {
                chunk_set.insert(c);
            }
        }
        // Q.4a: solidified cells are former lava cells now holding
        // obsidian / cobble. Treat them like placed entries for
        // chunk rebuild + mesh dirty so the renderer drops the
        // liquid slab and emits a full solid cube.
        for (p, _) in &tape.solidified {
            for c in Self::affected_chunks(*p) {
                chunk_set.insert(c);
            }
        }
        if !chunk_set.is_empty() {
            let coords: Vec<ChunkCoord> = chunk_set.into_iter().collect();
            self.rebuild_chunks(&coords);
        }
        if !tape.evaporated.is_empty()
            || !tape.placed.is_empty()
            || !tape.decayed.is_empty()
            || !tape.solidified.is_empty()
        {
            self.liquid_mesh_dirty = true;
        }
        // Broadcast edits + level updates over the wire.
        let cfg = bincode::config::standard();
        if let NetRole::Server { server, .. } = &mut self.net {
            for p in &tape.evaporated {
                let edit = EditMsg {
                    x: p[0],
                    y: p[1],
                    z: p[2],
                    block: 0,
                };
                self.edit_history.push(edit);
                if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                    server.broadcast_bytes("vx.edit", buf);
                }
            }
            for (p, block_id, level) in &tape.placed {
                let edit = EditMsg {
                    x: p[0],
                    y: p[1],
                    z: p[2],
                    block: *block_id,
                };
                self.edit_history.push(edit);
                if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                    server.broadcast_bytes("vx.edit", buf);
                }
                if let Ok(buf) = bincode::serde::encode_to_vec((*p, *level), cfg)
                {
                    server.broadcast_bytes("vx.liquid_lvl", buf);
                }
            }
            for (p, lvl) in &tape.decayed {
                if let Ok(buf) = bincode::serde::encode_to_vec((*p, *lvl), cfg) {
                    server.broadcast_bytes("vx.liquid_lvl", buf);
                }
            }
            // Q.4a: lava-water boundary -> broadcast the solid
            // block id as a regular edit so remote peers see the
            // obsidian / cobble swap. No liquid_lvl needed - the
            // cell is no longer a liquid.
            for (p, solid_id) in &tape.solidified {
                let edit = EditMsg {
                    x: p[0],
                    y: p[1],
                    z: p[2],
                    block: *solid_id,
                };
                self.edit_history.push(edit);
                if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                    server.broadcast_bytes("vx.edit", buf);
                }
            }
        }
        // MC: spread schedules the NEXT tick for each cell it
        // actually wrote into. A cell whose state stayed exactly
        // the same (apply collapsed the proposal into a no-op
        // because target was already at that level) doesn't get
        // re-scheduled - that's how settled pools quiesce.
        //
        // We mirror by only scheduling cells the tape ACTUALLY
        // touched + the 6 neighbours of those, so an outward wave
        // keeps marching and a settled pool stops ticking.
        for p in &tape.evaporated {
            for d in &SIX_NSEW_UD {
                let n = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
                if self.world.get(n).0 == liquid_id {
                    self.liquid_schedule.schedule(n, now + delay);
                }
            }
        }
        for (p, _, _) in &tape.placed {
            self.liquid_schedule.schedule(*p, now + delay);
            for d in &SIX_NSEW_UD {
                let n = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
                if self.world.get(n).0 == liquid_id {
                    self.liquid_schedule.schedule(n, now + delay);
                }
            }
        }
        for (p, _) in &tape.decayed {
            self.liquid_schedule.schedule(*p, now + delay);
            for d in &SIX_NSEW_UD {
                let n = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
                if self.world.get(n).0 == liquid_id {
                    self.liquid_schedule.schedule(n, now + delay);
                }
            }
        }
        // Q.4a: solidified cells leave the liquid pool entirely;
        // wake every adjacent liquid neighbour (water OR lava) so
        // the flow re-evaluates against the new solid block. MC
        // does this implicitly via `setBlockAndUpdate` which fires
        // neighborChanged.
        for (p, _) in &tape.solidified {
            for d in &SIX_NSEW_UD {
                let n = [p[0] + d[0], p[1] + d[1], p[2] + d[2]];
                let nb = self.world.get(n);
                if crate::block::block_is_liquid(nb) {
                    self.liquid_schedule.schedule(n, now + 1);
                }
            }
        }
    }


    /// Source-merge pass: a flow voxel of `liquid` adjacent to
    /// two or more same-kind source neighbours gets promoted to
    /// a source itself (MC's infinite-water trick). Only called
    /// for liquids whose `infinite_source` flag is set.
    ///
    /// Mirrors the reference `shouldBecomeSource +
    /// isStableForSourceConversion` pair: the promotion only
    /// fires when every NSEW neighbour is either a same-kind
    /// source or a solid wall - no drop and no open flow path
    /// out of the pocket. Prevents stray source spawns in
    /// chaotic configurations.
    fn tick_liquid_merge(&mut self, liquid: &Liquid) {
        if self.liquid_level.is_empty() {
            return;
        }
        let promoted = liquid_merge_step(&self.world, &self.liquid_level, liquid);
        if promoted.is_empty() {
            return;
        }
        for pos in &promoted {
            self.liquid_level.insert(*pos, 0);
        }
        self.liquid_mesh_dirty = true;
        let cfg = bincode::config::standard();
        if let NetRole::Server { server, .. } = &mut self.net {
            for pos in &promoted {
                if let Ok(buf) = bincode::serde::encode_to_vec((*pos, 0u8), cfg)
                {
                    server.broadcast_bytes("vx.liquid_lvl", buf);
                }
            }
        }
    }

    /// Server-side mob update: gravity-snap to surface, walk
    /// toward target, retarget on arrival / timeout, spawn new
    /// sheep when the cap allows. Builds the broadcast list as a
    /// side effect.
    /// Thin wrapper - see `mob_ai::tick_mobs`.
    fn tick_mobs(&mut self, dt: f32) {
        mob_ai::tick_mobs(self, dt);
    }

    /// Thin wrapper - see `mob_ai::tick_mob_audio`.
    fn tick_mob_audio(&mut self, dt: f32) {
        mob_ai::tick_mob_audio(self, dt);
    }

    fn sfx_at(
        &mut self,
        key: &str,
        pos: [f32; 3],
        synth: impl FnOnce(&mut infengine_audio::AudioSystem, f32),
    ) {
        const AUDIBLE_RANGE: f32 = 24.0;
        let Some(a) = self.audio.as_mut() else {
            return;
        };
        let listener = [self.player_pos.x, self.player_pos.y, self.player_pos.z];
        // Seed the variant shuffle from tick + spatial position so
        // back-to-back triggers don't reuse the same recording.
        let seed = self.net_tick.wrapping_mul(2654435761)
            ^ (pos[0].to_bits() as u64).wrapping_mul(7919)
            ^ (pos[2].to_bits() as u64);
        if self
            .audio_bank
            .play_at(a, key, listener, pos, AUDIBLE_RANGE, seed)
            .is_some()
        {
            return;
        }
        // Bank miss / out of range. For out-of-range we still want
        // the synth path to gate on distance, so reuse the same
        // formula here.
        let dx = pos[0] - listener[0];
        let dy = pos[1] - listener[1];
        let dz = pos[2] - listener[2];
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
        if dist >= AUDIBLE_RANGE {
            return;
        }
        let gain = (1.0 - dist / AUDIBLE_RANGE).clamp(0.0, 1.0);
        synth(a, gain);
    }


    /// Thin wrapper around `world_save::load_world`. Phase B.7a wired
    /// in the multi-world layer; this shim resolves the current slug,
    /// runs the optional one-shot legacy migration, then dispatches to
    /// the typed loader. A `NotFound` from the loader means the slot
    /// is fresh (no save yet) - in that case we just carry on with the
    /// procedurally generated world.
    fn load_server_state(&mut self) {
        // First-run legacy migration: if the pre-A4 single-file save
        // is still in CWD + the new worlds tree is empty, create a
        // `worlds/legacy/` slot.
        match world_save::migrate_legacy_if_needed() {
            Ok(Some(legacy_slug)) => {
                self.current_world_slug = Some(legacy_slug);
            }
            Ok(None) => {}
            Err(err) => warn!(error = %err, "world_save: legacy migration failed"),
        }
        let slug = self
            .current_world_slug
            .clone()
            .unwrap_or_else(|| world_save::DEFAULT_SLUG.to_string());
        match world_save::load_world(self, &slug) {
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                // Fresh slot - no save on disk yet. Carry on with the
                // procedurally generated world; the autosave loop will
                // mint the slot on the next save tick.
                self.current_world_slug = Some(slug);
            }
            Err(err) => warn!(error = %err, "world_save: load_world failed"),
        }
    }

    /// Raycast against every remote player's AABB. Returns the
    /// nearest hit `(id, distance)` within `max_dist`, or `None`.
    /// Skips our own id so we never target ourselves. Uses the
    /// interpolated render pose so the hit-test matches what the
    /// player actually sees on screen.
    fn raycast_remote_player(
        &self,
        origin: Vec3,
        dir: Vec3,
        max_dist: f32,
    ) -> Option<(u32, f32)> {
        let own_id: u32 = match &self.net {
            NetRole::Offline => return None,
            NetRole::Server { .. } => 0,
            NetRole::Client { client, .. } => client.client_id.unwrap_or(0),
        };
        let now = std::time::Instant::now();
        let mut best: Option<(u32, f32)> = None;
        for (&id, interp) in self.remote_interp.iter() {
            if id == own_id {
                continue;
            }
            let p = interp.sample(now);
            // Use the same body+head extents as the humanoid
            // render. Box centred at pose - (0, 0.7, 0).
            let cx = p.pos[0];
            let cy = p.pos[1] - 0.7;
            let cz = p.pos[2];
            let hx = 0.35;
            let hy = 1.0;
            let hz = 0.35;
            let min = [cx - hx, cy - hy, cz - hz];
            let max = [cx + hx, cy + hy, cz + hz];
            if let Some(t) = ray_aabb(origin.into(), dir.into(), min, max)
                && t <= max_dist
                && best.is_none_or(|(_, bt)| t < bt)
            {
                best = Some((id, t));
            }
        }
        best
    }

    /// Pick the nearest player position (host + remote pose
    /// table) within `range` of `from`. Returns `None` if no
    /// player is in range or there are no players known. Used by
    /// zombie AI to pick a chase target.
    fn nearest_player_pos(&self, from: [f32; 3], range: f32) -> Option<[f32; 3]> {
        let r2 = range * range;
        let mut best: Option<([f32; 3], f32)> = None;
        let mut consider = |p: [f32; 3]| {
            let dx = p[0] - from[0];
            let dy = p[1] - from[1];
            let dz = p[2] - from[2];
            let d2 = dx * dx + dy * dy + dz * dz;
            if d2 <= r2 && best.is_none_or(|(_, b)| d2 < b) {
                best = Some((p, d2));
            }
        };
        // Host's own pose is always in play.
        consider([self.player_pos.x, self.player_pos.y, self.player_pos.z]);
        // Plus every remote player tracked by the server pose
        // table (clients don't have other peers' poses authoritatively).
        if let NetRole::Server { poses, .. } = &self.net {
            for p in poses.values() {
                if p.id == 0 {
                    continue;
                }
                consider(p.pos);
            }
        }
        best.map(|(p, _)| p)
    }

    /// Probe the voxel column directly under the player's feet
    /// at world XZ `(wx, wz)` and return true if any cell across
    /// the player's collision footprint is solid. Used by the
    /// sneak edge-clamp so a grounded sneaking player can't walk
    /// off the side of a block.
    fn has_floor_under(&self, wx: f32, wz: f32) -> bool {
        let foot_y = (self.player_pos.y - PLAYER_HEIGHT - 0.05).floor() as i32;
        // The player AABB is `(wx - r, wz - r) -> (wx + r, wz + r)`
        // at foot height. Check the 4 corner cells (or fewer when
        // the player straddles fewer voxels) so we don't miss the
        // diagonal case where the supporting block sits on one
        // corner only.
        let r = PLAYER_RADIUS - COLLISION_EPSILON;
        let xs = [(wx - r).floor() as i32, (wx + r).floor() as i32];
        let zs = [(wz - r).floor() as i32, (wz + r).floor() as i32];
        for x in xs {
            for z in zs {
                if !self.world.get([x, foot_y, z]).is_air() {
                    return true;
                }
            }
        }
        false
    }

    /// Apply HP damage to the local player. Thin wrapper around
    /// `player::apply_damage` so existing call sites
    /// (`self.apply_player_damage(...)`) keep working.
    fn apply_player_damage(&mut self, amount: f32) {
        player::apply_damage(self, amount);
    }

    /// Variant that tags the source of the damage. Thin wrapper
    /// around `player::apply_damage_kind`.
    fn apply_player_damage_kind(&mut self, amount: f32, kind: DamageKind) {
        player::apply_damage_kind(self, amount, kind);
    }

    /// Fire-and-forget `vx.leave` to the server (if any). Called
    /// on explicit disconnect / quit so the server doesn't have
    /// to wait its 5 s silence reaper to broadcast a removal.
    fn notify_server_leaving(&self) {
        if let NetRole::Client { client, .. } = &self.net {
            let _ = client.send_bytes("vx.leave", Vec::new());
        }
    }

    /// Resolve the config file path used for settings persistence.
    /// Lives next to the exe in release builds so the user can
    /// nuke the file without finding a hidden appdata directory;
    /// during `cargo run` it sits next to the workspace target so
    /// repeated runs share the same file.
    fn config_path() -> std::path::PathBuf {
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            return dir.join("voxel_demo_mp.config");
        }
        std::path::PathBuf::from("voxel_demo_mp.config")
    }

    /// Resolve the optional message-of-the-day file path. Lives
    /// next to the exe so a host can hand-edit `motd.txt`. None
    /// when the file isn't present.
    fn motd_lines() -> Vec<String> {
        let path = if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            dir.join("motd.txt")
        } else {
            std::path::PathBuf::from("motd.txt")
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => text
                .lines()
                .filter(|l| !l.trim().is_empty())
                .take(8)
                .map(|s| s.to_string())
                .collect(),
            Err(_) => Vec::new(),
        }
    }

    /// Persist user-facing settings (name, sensitivity, toggles)
    /// to disk as a tiny key=value text file. Called from the
    /// Settings panel's Back button + on shutdown so the next run
    /// keeps the player's identity.
    fn save_config(&self) {
        use std::fmt::Write;
        let mut text = String::new();
        let _ = writeln!(text, "player_name={}", self.player_name);
        let _ = writeln!(
            text,
            "mouse_sensitivity={}",
            self.settings.mouse_sensitivity
        );
        let _ = writeln!(text, "show_fps={}", self.settings.show_fps);
        let _ = writeln!(text, "lock_cursor={}", self.settings.lock_cursor);
        let _ = writeln!(text, "hide_cursor={}", self.settings.hide_cursor);
        let _ = writeln!(text, "view_distance={}", self.settings.view_distance);
        let _ = writeln!(text, "head_bob={}", self.settings.head_bob);
        let _ = writeln!(
            text,
            "invert_mouse_y={}",
            self.settings.invert_mouse_y
        );
        let _ = writeln!(
            text,
            "invert_wheel={}",
            self.settings.invert_wheel
        );
        if let Some(p) = self.home_pos {
            let _ = writeln!(text, "home={},{},{}", p[0], p[1], p[2]);
        }
        if let Some(p) = self.spawn_pos {
            let _ = writeln!(text, "spawn={},{},{}", p[0], p[1], p[2]);
        }
        // Optional debug toggles persisted so a dev workflow
        // doesn't have to retype /fly /godmode each launch.
        if self.fly_mode {
            let _ = writeln!(text, "fly_mode=true");
        }
        if self.god_mode {
            let _ = writeln!(text, "god_mode=true");
        }
        // World seed persists for solo play so re-runs share
        // the same terrain. Hosted sessions still rely on
        // ServerStateV2's own seed field.
        let _ = writeln!(text, "world_seed={}", world_seed());
        let path = Self::config_path();
        if let Err(err) = std::fs::write(&path, text) {
            warn!(error = %err, path = %path.display(), "config save failed");
        }
    }

    /// Inverse of `save_config`. Silently ignores missing /
    /// malformed files - first launch just writes defaults the
    /// next time the user opens Settings.
    fn load_config(&mut self) {
        let path = Self::config_path();
        let Ok(text) = std::fs::read_to_string(&path) else {
            return;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            match key {
                "player_name" => {
                    let sane: String = value
                        .chars()
                        .filter(|c| !c.is_control())
                        .take(24)
                        .collect();
                    if !sane.is_empty() {
                        self.player_name = sane;
                    }
                }
                "mouse_sensitivity" => {
                    if let Ok(v) = value.parse::<f32>() {
                        self.settings.mouse_sensitivity = v.clamp(0.05, 4.0);
                    }
                }
                "show_fps" => {
                    self.settings.show_fps = matches!(value, "true" | "1" | "yes");
                }
                "lock_cursor" => {
                    self.settings.lock_cursor =
                        matches!(value, "true" | "1" | "yes");
                }
                "hide_cursor" => {
                    self.settings.hide_cursor =
                        matches!(value, "true" | "1" | "yes");
                }
                "view_distance" => {
                    if let Ok(v) = value.parse::<u8>() {
                        self.settings.view_distance = v.clamp(2, 12);
                    }
                }
                "head_bob" => {
                    self.settings.head_bob =
                        matches!(value, "true" | "1" | "yes");
                }
                "invert_mouse_y" => {
                    self.settings.invert_mouse_y =
                        matches!(value, "true" | "1" | "yes");
                }
                "invert_wheel" => {
                    self.settings.invert_wheel =
                        matches!(value, "true" | "1" | "yes");
                }
                "home" => {
                    let parts: Vec<&str> = value.split(',').collect();
                    if parts.len() == 3
                        && let (Ok(x), Ok(y), Ok(z)) = (
                            parts[0].trim().parse::<f32>(),
                            parts[1].trim().parse::<f32>(),
                            parts[2].trim().parse::<f32>(),
                        )
                    {
                        self.home_pos = Some([x, y, z]);
                    }
                }
                "spawn" => {
                    let parts: Vec<&str> = value.split(',').collect();
                    if parts.len() == 3
                        && let (Ok(x), Ok(y), Ok(z)) = (
                            parts[0].trim().parse::<f32>(),
                            parts[1].trim().parse::<f32>(),
                            parts[2].trim().parse::<f32>(),
                        )
                    {
                        self.spawn_pos = Some([x, y, z]);
                    }
                }
                "fly_mode" => {
                    self.fly_mode = matches!(value, "true" | "1" | "yes");
                }
                "god_mode" => {
                    self.god_mode = matches!(value, "true" | "1" | "yes");
                }
                "world_seed" => {
                    if let Ok(v) = value.parse::<u32>()
                        && v != 0
                    {
                        set_world_seed(v);
                    }
                }
                _ => {}
            }
        }
    }

    /// Notify the player visually that an autosave just landed.
    /// A short-lived chat line is the cheapest path that doesn't
    /// require a new HUD layer.
    fn note_save(&mut self) {
        self.chat_log.push((
            std::time::Instant::now(),
            "[autosave] World state saved.".to_string(),
        ));
    }

    /// Thin wrapper around `world_save::save_world`. Phase B.7a wired
    /// in the multi-world layer; this shim resolves the slug + gates
    /// on `Server` / `Offline` (clients are never authoritative).
    fn save_server_state(&mut self) {
        // Only the host writes saves; clients see snapshots on join.
        if !matches!(self.net, NetRole::Server { .. } | NetRole::Offline) {
            return;
        }
        let slug = self
            .current_world_slug
            .clone()
            .unwrap_or_else(|| world_save::DEFAULT_SLUG.to_string());
        if let Err(err) = world_save::save_world(self, &slug) {
            warn!(error = %err, "world_save: save_world failed");
        }
    }

    /// Push a small burst of debris particles into the system at the
    /// centre of `voxel`. Used as a "block broken" effect from every
    /// edit-apply path so both the player and remote players see
    /// puffs when something mines.
    fn spawn_break_particles(&mut self, voxel: [i32; 3]) {
        // No-tint variant: callers that don't know the source
        // block fall through to a neutral dirt-ish color.
        self.spawn_break_particles_tinted(voxel, [0.75, 0.55, 0.30]);
    }

    /// Empty-bucket RMB scoop. Casts a ray that stops at the
    /// first liquid voxel or solid block, accepts a liquid
    /// SOURCE (level 0), swaps the bucket to its filled
    /// variant, clears the source voxel, and broadcasts the
    /// edit. Returns `true` when the scoop succeeded.
    ///
    /// Runs OUTSIDE the placement-target chain because
    /// `self.target` skips `F_RAY_PASS` liquids - if the player
    /// aims an empty bucket at a water source with sky behind
    /// it, the placement ray returns `None` and the chain's
    /// bucket arm never fires. This dedicated raycast handles
    /// the scoop independently.
    fn try_scoop_with_bucket(&mut self, forward: Vec3) -> bool {
        let liquid_hit = raycast_with(
            &self.world,
            self.player_pos,
            forward,
            6.0,
            |b| {
                !b.is_air()
                    && (block_is_liquid(b) || !block_is_ray_passable(b))
            },
        );
        let Some(scoop_voxel) = liquid_hit.map(|h| h.voxel) else {
            return false;
        };
        let scoop_block = self.world.get(scoop_voxel);
        if !block_is_liquid(scoop_block) {
            return false;
        }
        // MC: only source voxels (`isSource()`) can be bottled.
        // Flow / falling cells pop the bucket back empty.
        let scoop_level = self
            .liquid_level
            .get(&scoop_voxel)
            .copied()
            .unwrap_or(0);
        if scoop_level != 0 {
            return false;
        }
        let Some(filled) = liquid_to_bucket_id(scoop_block.0) else {
            return false;
        };
        let cfg = bincode::config::standard();
        self.world_set_tracked(scoop_voxel, BlockId::AIR);
        let coords = Self::affected_chunks(scoop_voxel);
        self.rebuild_chunks(&coords);
        self.inventory[self.selected_slot] = Some((BlockId(filled), 1));
        let edit = EditMsg {
            x: scoop_voxel[0],
            y: scoop_voxel[1],
            z: scoop_voxel[2],
            block: 0,
        };
        match &mut self.net {
            NetRole::Offline => {}
            NetRole::Server { server, .. } => {
                self.edit_history.push(edit);
                if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                    server.broadcast_bytes("vx.edit", buf);
                }
            }
            NetRole::Client { pending_edits, .. } => {
                pending_edits.push(edit);
            }
        }
        let center = [
            scoop_voxel[0] as f32 + 0.5,
            scoop_voxel[1] as f32 + 0.5,
            scoop_voxel[2] as f32 + 0.5,
        ];
        self.sfx_at("splash", center, |a, g| {
            let _ = a.play_thud(140.0, 0.18 * g);
            let _ = a.play_swish(0.16 * g);
        });
        true
    }

    /// Tinted variant. `color` is the linear-RGB albedo of the
    /// broken block; the debris cubes pick up that hue with a
    /// small per-particle brightness jitter so they read as
    /// material-matched chips instead of a generic puff.
    fn spawn_break_particles_tinted(&mut self, voxel: [i32; 3], color: [f32; 3]) {
        let pos = [
            voxel[0] as f32 + 0.5,
            voxel[1] as f32 + 0.5,
            voxel[2] as f32 + 0.5,
        ];
        let mut rng: u32 = 0xCAFEBABE
            ^ (voxel[0] as u32)
            ^ ((voxel[1] as u32) << 7)
            ^ ((voxel[2] as u32) << 13);
        let mut rand01 = || -> f32 {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            (rng as f32 / u32::MAX as f32).clamp(0.0, 1.0)
        };
        for _ in 0..12 {
            let vx = (rand01() * 2.0 - 1.0) * 3.0;
            let vy = rand01() * 3.5;
            let vz = (rand01() * 2.0 - 1.0) * 3.0;
            // Small per-particle brightness jitter so the puff
            // doesn't read as a flat monochrome cluster.
            let j = 0.85 + rand01() * 0.30;
            self.particles.particles.push(infengine_particles::Particle {
                position: pos,
                velocity: [vx, vy, vz],
                color: [
                    (color[0] * j).clamp(0.0, 1.0),
                    (color[1] * j).clamp(0.0, 1.0),
                    (color[2] * j).clamp(0.0, 1.0),
                    1.0,
                ],
                size: 0.12,
                age: 0.0,
                lifetime: 0.5,
            });
        }
    }

    /// Burst of small dust particles when a block is placed. Half
    /// the count of `spawn_break_particles` so it reads as a
    /// gentle "click" rather than a full debris cloud, tinted with
    /// the block's albedo.
    fn spawn_place_particles(&mut self, voxel: [i32; 3], color: [f32; 3]) {
        let pos = [
            voxel[0] as f32 + 0.5,
            voxel[1] as f32 + 0.5,
            voxel[2] as f32 + 0.5,
        ];
        let mut rng: u32 = 0xABAD1DEA
            ^ (voxel[0] as u32)
            ^ ((voxel[1] as u32) << 7)
            ^ ((voxel[2] as u32) << 13);
        let mut rand01 = || -> f32 {
            rng ^= rng << 13;
            rng ^= rng >> 17;
            rng ^= rng << 5;
            (rng as f32 / u32::MAX as f32).clamp(0.0, 1.0)
        };
        for _ in 0..6 {
            let vx = (rand01() * 2.0 - 1.0) * 1.5;
            let vy = rand01() * 1.0 + 0.5;
            let vz = (rand01() * 2.0 - 1.0) * 1.5;
            self.particles.particles.push(infengine_particles::Particle {
                position: pos,
                velocity: [vx, vy, vz],
                color: [color[0], color[1], color[2], 0.9],
                size: 0.08,
                age: 0.0,
                lifetime: 0.3,
            });
        }
    }

    /// Add one `block` to the inventory, Minecraft-style. The
    /// algorithm walks the whole inventory:
    ///
    /// 1. Find an existing non-full stack of the same block. Bump
    ///    its count and return.
    /// 2. Otherwise drop a fresh `(block, 1)` into the first empty
    ///    slot.
    /// 3. If everything is full, silently discard (no item entities
    ///    in this demo yet).
    ///
    /// `BlockId::AIR` is a no-op.
    fn pickup(&mut self, block: BlockId) {
        if block == BlockId::AIR {
            return;
        }
        // First pass: top up an existing stack.
        for slot in self.inventory.iter_mut() {
            if let Some((b, count)) = slot.as_mut()
                && *b == block
                && *count < STACK_MAX
            {
                *count += 1;
                self.note_pickup(block);
                return;
            }
        }
        // Second pass: claim an empty slot. Prefer hotbar first so
        // freshly mined blocks become hot-keyable immediately.
        for slot in self.inventory.iter_mut() {
            if slot.is_none() {
                *slot = Some((block, 1));
                self.note_pickup(block);
                return;
            }
        }
    }

    /// Register a successful pickup for the floating "+1 wood"
    /// toast HUD. Collapses adjacent picks of the same block into
    /// a single entry so spamming the same stack doesn't bury the
    /// HUD in a wall of one-liners.
    fn note_pickup(&mut self, block: BlockId) {
        let now = std::time::Instant::now();
        if let Some(last) = self.pickup_toasts.last_mut()
            && last.1 == block
            && now.duration_since(last.0).as_secs_f32() < 1.5
        {
            last.0 = now;
            last.2 = last.2.saturating_add(1);
            return;
        }
        self.pickup_toasts.push((now, block, 1));
        // Cap the buffer so a long mining session doesn't grow
        // unbounded. Oldest entries are the first to roll off the
        // top of the HUD anyway.
        if self.pickup_toasts.len() > 16 {
            let drop = self.pickup_toasts.len() - 16;
            self.pickup_toasts.drain(0..drop);
        }
    }

    /// Read the (BlockId, count) currently sitting in `kind`. The
    /// crafting output is virtual: it's whatever recipe matches the
    /// current 2x2 grid (or `None` if no match).
    fn slot_read(&self, kind: SlotKind) -> Option<(BlockId, u8)> {
        match kind {
            SlotKind::Inv(i) => self.inventory.get(i).copied().flatten(),
            SlotKind::Craft(i) => self.craft_grid.get(i).copied().flatten(),
            SlotKind::CraftOut => match_recipe(&self.craft_grid),
        }
    }

    /// Write `value` into `kind`. Never writes to `CraftOut`
    /// (that's a virtual cell - taking from it goes through
    /// `consume_craft` instead).
    fn slot_write(&mut self, kind: SlotKind, value: Option<(BlockId, u8)>) {
        match kind {
            SlotKind::Inv(i) => {
                if i < self.inventory.len() {
                    self.inventory[i] = value;
                }
            }
            SlotKind::Craft(i) => {
                if i < self.craft_grid.len() {
                    self.craft_grid[i] = value;
                }
            }
            SlotKind::CraftOut => {}
        }
    }

    /// Drain one item from every populated craft cell. Called when
    /// the player takes the craft output.
    fn consume_craft(&mut self) {
        for slot in self.craft_grid.iter_mut() {
            if let Some((_, c)) = slot.as_mut() {
                *c = c.saturating_sub(1);
                if *c == 0 {
                    *slot = None;
                }
            }
        }
    }

    /// Dispatch one inventory-UI action. Drag&drop semantics: a
    /// drag from `from` to `to` either swaps the two stacks or
    /// (when the blocks match) merges into the destination.
    /// Hotbar slot 0 is the break-tool slot and refuses incoming
    /// stacks so its X icon never disappears.
    fn apply_inv_action(&mut self, action: InvAction) {
        match action {
            InvAction::SelectHotbar(idx) => {
                if idx < HOTBAR_SLOTS {
                    self.selected_slot = idx;
                }
            }
            InvAction::Drag { from, to, split } => {
                if from == to {
                    return;
                }
                // Can't drop INTO the crafting output - it's a
                // virtual cell.
                if matches!(to, SlotKind::CraftOut) {
                    return;
                }
                // Protect the break-tool slot.
                if matches!(to, SlotKind::Inv(0)) {
                    return;
                }
                let Some(from_stack) = self.slot_read(from) else {
                    return;
                };
                let to_stack = self.slot_read(to);
                let craft_take = matches!(from, SlotKind::CraftOut);
                // CraftOut drag has special rules: must land in an
                // empty slot or merge into a same-block stack with
                // room for the full output. Splitting is a no-op
                // here - the output is a single fixed packet.
                if craft_take {
                    match to_stack {
                        None => {
                            self.consume_craft();
                            self.slot_write(to, Some(from_stack));
                        }
                        Some((tb, tc))
                            if tb == from_stack.0
                                && (tc as u16 + from_stack.1 as u16) <= STACK_MAX as u16 =>
                        {
                            self.consume_craft();
                            self.slot_write(to, Some((tb, tc + from_stack.1)));
                        }
                        _ => {}
                    }
                    return;
                }
                // Partial move (`SplitMode::Half` or `One`).
                // `All` falls through to the whole-stack swap path.
                let take = match split {
                    SplitMode::All => 0u8,
                    SplitMode::Half => (from_stack.1 as u16).div_ceil(2) as u8,
                    SplitMode::One => 1u8,
                };
                if take > 0 {
                    let take = take.max(1).min(from_stack.1);
                    let rem = from_stack.1 - take;
                    match to_stack {
                        Some((tb, tc)) if tb == from_stack.0 => {
                            let merge = (STACK_MAX - tc).min(take);
                            let new_to = tc + merge;
                            let leftover = take - merge;
                            self.slot_write(to, Some((tb, new_to)));
                            // Any leftover from the take goes back
                            // into the source slot.
                            let back = rem + leftover;
                            self.slot_write(
                                from,
                                if back == 0 {
                                    None
                                } else {
                                    Some((from_stack.0, back))
                                },
                            );
                        }
                        None => {
                            self.slot_write(to, Some((from_stack.0, take)));
                            self.slot_write(
                                from,
                                if rem == 0 {
                                    None
                                } else {
                                    Some((from_stack.0, rem))
                                },
                            );
                        }
                        Some(_) => {
                            // Target occupied by a different block:
                            // a split into an unrelated slot would
                            // either swap (loses semantics) or
                            // refuse. We refuse.
                        }
                    }
                    return;
                }
                // Standard whole-stack drag between two real slots.
                match to_stack {
                    Some((tb, tc)) if tb == from_stack.0 => {
                        // Same block -> merge.
                        let take = (STACK_MAX - tc).min(from_stack.1);
                        let new_to = tc + take;
                        let rem = from_stack.1 - take;
                        self.slot_write(to, Some((tb, new_to)));
                        self.slot_write(
                            from,
                            if rem == 0 { None } else { Some((from_stack.0, rem)) },
                        );
                    }
                    _ => {
                        // Different blocks (or empty target) -> swap.
                        self.slot_write(to, Some(from_stack));
                        self.slot_write(from, to_stack);
                    }
                }
            }
        }
    }

    /// Spawn a NetClient at `addr` and transition into Playing.
    fn connect_as_client(&mut self, addr: &str) {
        // Sanitize: trimmed + non-empty so the server never sees
        // a blank Hello name. Falls back to a session-anchored
        // pseudo-random `Player####` when the user cleared the
        // field in settings.
        let trimmed: String = self
            .player_name
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(24)
            .collect();
        if trimmed.is_empty() {
            let n = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
                % 10000;
            self.player_name = format!("Player{n:04}");
        } else if trimmed != self.player_name {
            self.player_name = trimmed;
        }
        match NetClient::connect(addr, &self.player_name) {
            Ok(c) => {
                info!(addr = %addr, "joining server");
                self.net = NetRole::Client {
                    client: c,
                    remote: HashMap::new(),
                    pending_edits: Vec::new(),
                    pending_inv_actions: Vec::new(),
                };
                self.state = AppState::Playing;
                self.mouse_dx = 0.0;
                self.mouse_dy = 0.0;
            }
            Err(err) => {
                warn!(error = %err, addr = %addr, "client connect failed");
                self.browser_error = Some(format!("connect failed: {err}"));
            }
        }
    }

    /// Bind a NetServer on the configured port, optionally announce
    /// to the master server, then drop into Playing.
    fn start_hosting(&mut self) {
        let port: u16 = self.host_port_buf.parse().unwrap_or(7777);
        let bind = format!("0.0.0.0:{port}");
        match NetServer::bind(&bind) {
            Ok(s) => {
                info!(addr = %bind, "hosting voxel server");
                // Pick a world seed for this session if one
                // wasn't restored from the on-disk state. The
                // nanosecond clock + a fixed mixing constant give
                // a different terrain layout per cold start.
                if world_seed() == 0 {
                    let seed = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.subsec_nanos())
                        .unwrap_or(0)
                        .wrapping_mul(0x9E37_79B1)
                        | 1;
                    set_world_seed(seed);
                }
                self.net = NetRole::Server {
                    server: s,
                    poses: HashMap::new(),
                    inventories: HashMap::new(),
                    pending_inv_sends: HashSet::new(),
                };
                self.state = AppState::Playing;
                self.mouse_dx = 0.0;
                self.mouse_dy = 0.0;
                if self.host_announce {
                    self.announce_to_master(port);
                }
            }
            Err(err) => {
                warn!(error = %err, port, "server bind failed");
                self.browser_error = Some(format!("host failed: {err}"));
            }
        }
    }

    /// Register this host with the configured master server. Caches
    /// the returned token so periodic heartbeats land on the right
    /// directory entry.
    fn announce_to_master(&mut self, port: u16) {
        use infengine_net::master::{MASTER_PROTO, MasterRequest, ServerInfo};
        if self.announce_query_rx.is_some() {
            return; // in-flight already
        }
        let info = ServerInfo {
            name: self.host_name_buf.clone(),
            addr: format!("0.0.0.0:{port}"),
            players: 0,
            max_players: 16,
            mode: "voxel".into(),
        };
        let req = MasterRequest::Register {
            proto: MASTER_PROTO,
            info,
        };
        let addr = self.master_addr.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use infengine_net::master::{MasterResponse, request_blocking};
            let result =
                match request_blocking(&addr, &req, std::time::Duration::from_secs(2)) {
                    Some(MasterResponse::Registered { token }) => Ok(token),
                    Some(MasterResponse::Error { message }) => {
                        Err(format!("master: {message}"))
                    }
                    _ => Err("master timeout".into()),
                };
            let _ = tx.send(result);
        });
        self.announce_query_rx = Some(rx);
    }

    /// Fetch the server list from the master and stash it for the
    /// browser UI to render.
    fn refresh_browser(&mut self) {
        use infengine_net::master::{MASTER_PROTO, MasterRequest};
        if self.browser_query_rx.is_some() {
            return; // already in flight
        }
        self.browser_error = None;
        let req = MasterRequest::ListServers {
            proto: MASTER_PROTO,
        };
        let addr = self.master_addr.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            use infengine_net::master::{MasterResponse, request_blocking};
            let result =
                match request_blocking(&addr, &req, std::time::Duration::from_secs(2)) {
                    Some(MasterResponse::List { servers }) => Ok(servers),
                    Some(MasterResponse::Error { message }) => {
                        Err(format!("master: {message}"))
                    }
                    _ => Err("master timeout".into()),
                };
            let _ = tx.send(result);
        });
        self.browser_query_rx = Some(rx);
    }

    /// Drain any completed background master queries. Called every
    /// tick from `on_update` so async results land back on the UI
    /// without polling tight loops.
    fn poll_master_queries(&mut self) {
        use std::sync::mpsc::TryRecvError;
        let browser_done = if let Some(rx) = &self.browser_query_rx {
            match rx.try_recv() {
                Ok(Ok(servers)) => {
                    info!(count = servers.len(), "fetched server list");
                    self.browser_servers = servers;
                    self.browser_error = None;
                    self.browser_last_refresh = Some(std::time::Instant::now());
                    true
                }
                Ok(Err(err)) => {
                    warn!(error = %err, "master list failed");
                    self.browser_error = Some(err);
                    self.browser_last_refresh = Some(std::time::Instant::now());
                    true
                }
                Err(TryRecvError::Empty) => false,
                Err(TryRecvError::Disconnected) => true,
            }
        } else {
            false
        };
        if browser_done {
            self.browser_query_rx = None;
        }
        let announce_done = if let Some(rx) = &self.announce_query_rx {
            match rx.try_recv() {
                Ok(Ok(token)) => {
                    info!(token, "announced to master");
                    self.master_token = Some(token);
                    self.last_heartbeat = Some(std::time::Instant::now());
                    self.browser_error = None;
                    true
                }
                Ok(Err(err)) => {
                    warn!(error = %err, "announce failed");
                    self.browser_error = Some(err);
                    true
                }
                Err(TryRecvError::Empty) => false,
                Err(TryRecvError::Disconnected) => true,
            }
        } else {
            false
        };
        if announce_done {
            self.announce_query_rx = None;
        }
    }

    /// Per-tick networking step:
    /// - Server: poll, drain client edits + poses, apply each edit
    ///   locally, broadcast applied edits, broadcast every player's
    ///   latest pose.
    /// - Client: poll, drain server snapshots, apply received edits,
    ///   update remote-player table, flush pending edits + own pose
    ///   upstream.
    fn tick_net(&mut self) {
        self.net_tick = self.net_tick.wrapping_add(1);
        let net_tick = self.net_tick;
        let player_pos = self.player_pos;
        let player_yaw = self.player_yaw;
        let player_pitch = self.player_pitch;
        // Retransmit period (in net ticks) for pending unsynced
        // payloads. UDP losses are rare on LAN so resending every
        // tick is wasteful; every 4 ticks is roughly 67 ms at
        // 60 Hz which still feels snappy.
        const RETRY_PERIOD: u64 = 4;
        let do_retry = net_tick.is_multiple_of(RETRY_PERIOD);
        // Server-side heartbeat to the master server. Cheap so we
        // can do it inline.
        if let NetRole::Server { server, poses, .. } = &self.net
            && let Some(token) = self.master_token
            && self
                .last_heartbeat
                .is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(5))
        {
            use infengine_net::master::{MasterRequest, request_blocking};
            let players = poses.len().saturating_sub(1) as u32 + server.client_count() as u32;
            let req = MasterRequest::Heartbeat { token, players };
            let addr = self.master_addr.clone();
            // Fire-and-forget so the server tick never stalls on a
            // network round trip. We don't care about the response;
            // a stale token just causes the next heartbeat to log
            // a warning.
            std::thread::spawn(move || {
                let _ = request_blocking(&addr, &req, std::time::Duration::from_millis(500));
            });
            self.last_heartbeat = Some(std::time::Instant::now());
        }
        // Drain incoming bytes + edits to apply. Collect first so the
        // borrow on `self.net` releases before we mutate the world.
        let mut to_apply: Vec<EditMsg> = Vec::new();
        // Validated grass voxels from inbound vx.path requests.
        // Filled in the Server arm + drained after self.net's
        // borrow releases.
        let mut path_apply: Vec<[i32; 3]> = Vec::new();
        // Inventory snapshot the client received this tick. Applied
        // to `self.inventory` / `self.craft_grid` after the
        // `&mut self.net` borrow releases.
        let mut client_snapshot: Option<InvSnapshotMsg> = None;
        // Mob pose updates received this tick (client). Folded into
        // `self.remote_mobs` after the match exits.
        let mut mob_updates: Vec<MobMsg> = Vec::new();
        let mut player_removals: Vec<u32> = Vec::new();
        // Pose updates collected this tick. Folded into the
        // smoothing buffer (`remote_interp`) after the net match
        // arm releases its borrow on `self.net`.
        let mut interp_updates: Vec<PlayerMsg> = Vec::new();
        // Local damage accumulator drained after the match arm
        // releases its borrow on `self.net`. Used by:
        //  - vx.damage (client side): server-pushed PvP damage.
        //  - vx.pvp_hit (server side, host = target): a remote
        //    client whacked the host.
        let mut pvp_self_damage: f32 = 0.0;
        let mut pvp_self_heal: f32 = 0.0;
        // Chat lines received this tick. Appended to `chat_log`
        // after the net match arm releases its borrow on `self.net`.
        let mut chat_inbox: Vec<String> = Vec::new();
        // Roster update from the server's `vx.names` broadcast.
        // Last-write-wins per tick: only the latest snapshot is
        // applied to `player_names` after the match arm closes.
        let mut names_update: Option<Vec<(u32, String)>> = None;
        // Server-pushed teleport. Drained after the net borrow
        // releases so we can write `self.player_pos` directly.
        let mut teleport_request: Option<TeleportMsg> = None;
        // Latest authoritative day_time from the server. Last
        // wins so a mid-tick burst doesn't bounce the sky.
        let mut day_time_update: Option<f32> = None;
        let mut seed_update: Option<u32> = None;
        let mut weather_update: Option<u8> = None;
        let mut snow_updates: Vec<([i32; 3], u8)> = Vec::new();
        // Liquid level updates received this tick. Drained into
        // `self.liquid_level` after the net match arm releases
        // its borrow on `self.net`. Client-only path: the host
        // sets the level directly via the tick passes; the
        // broadcast is only consumed by clients so their slope
        // mesh matches the host's depth ramp.
        let mut liquid_level_updates: Vec<([i32; 3], u8)> = Vec::new();
        // Drop entity updates received this tick. Client-only -
        // the host is the source of truth and never receives
        // these. Drained into `self.dropped_items` after the
        // net match arm releases its borrow on `self.net`.
        let mut drop_spawns: Vec<DropSpawnMsg> = Vec::new();
        let mut drop_removes: Vec<u32> = Vec::new();
        // Torch lean orientation updates received this tick. Drained
        // into `self.torch_orient` after the net match arm releases
        // its borrow on `self.net`. Server arm also rebroadcasts so
        // every peer renders the matching wall mount.
        let mut torch_orient_updates: Vec<([i32; 3], u8)> = Vec::new();
        let mut kick_reason: Option<String> = None;
        // Mob damage requests + events to process after the match
        // arm closes (damage_mob borrows the whole `&mut self`).
        let mut mob_hits_to_apply: Vec<(u32, u32)> = Vec::new();
        let mut mob_events: Vec<MobEventMsg> = Vec::new();
        let mut mob_removals: Vec<u32> = Vec::new();
        let cfg = bincode::config::standard();
        match &mut self.net {
            NetRole::Offline => return,
            NetRole::Server {
                server,
                poses,
                inventories,
                pending_inv_sends,
            } => {
                server.poll();
                // Track host pose under id 0.
                poses.insert(
                    0,
                    PlayerMsg {
                        id: 0,
                        pos: [player_pos.x, player_pos.y, player_pos.z],
                        yaw: player_yaw,
                        pitch: player_pitch,
                    },
                );
                // Raw inbound per client: (client_id, edit) +
                // (client_id, drag action). Resolved after the
                // drain loop so we can validate against the
                // authoritative inventory.
                let mut raw_edits: Vec<(u32, EditMsg)> = Vec::new();
                let mut path_requests: Vec<(u32, [i32; 3])> = Vec::new();
                let mut raw_drags: Vec<(u32, InvActionMsg)> = Vec::new();
                {
                    let mut inbox = net::ServerInboxCtx {
                        day_time: &mut self.day_time,
                        last_day_broadcast: &mut self.last_day_broadcast,
                        world_locked: self.world_locked,
                        host_player_pos: [
                            player_pos.x,
                            player_pos.y,
                            player_pos.z,
                        ],
                        raw_edits: &mut raw_edits,
                        path_requests: &mut path_requests,
                        raw_drags: &mut raw_drags,
                        interp_updates: &mut interp_updates,
                        mob_hits_to_apply: &mut mob_hits_to_apply,
                        torch_orient_updates: &mut torch_orient_updates,
                        chat_inbox: &mut chat_inbox,
                        pvp_self_damage: &mut pvp_self_damage,
                    };
                    net::process_server_inbox(
                        server,
                        poses,
                        inventories,
                        pending_inv_sends,
                        PVP_REACH,
                        PVP_DAMAGE,
                        DAY_LENGTH_S,
                        &|idx| RECIPES.get(idx).map(|r| r.inputs),
                        &mut inbox,
                    );
                }
                // Roster snapshot: use the authoritative connection
                // list (`server.client_ids`) rather than the pose
                // table, so dropped clients are detected as soon as
                // the reaper or an explicit `vx.leave` removes them
                // from the socket layer.
                let current_ids: HashSet<u32> =
                    server.client_ids().into_iter().collect();
                let new_ids: Vec<u32> = current_ids
                    .iter()
                    .filter(|id| !self.known_client_ids.contains(*id))
                    .copied()
                    .collect();
                let departed_ids: Vec<u32> = self
                    .known_client_ids
                    .iter()
                    .filter(|id| !current_ids.contains(*id))
                    .copied()
                    .collect();
                // Drop server-side state for clients that left so
                // their pose / inventory don't keep replicating.
                // Persist their inventory by name (if known) so the
                // next save flushes it, then evict.
                for id in &departed_ids {
                    // Grab the name BEFORE we lose track of the
                    // mapping below so the broadcast chat line
                    // reads sensibly.
                    let depart_name = self
                        .player_names
                        .get(id)
                        .cloned()
                        .or_else(|| server.client_name(*id))
                        .unwrap_or_else(|| format!("p{id}"));
                    poses.remove(id);
                    if let Some(inv) = inventories.remove(id)
                        && let Some(name) = server.client_name(*id)
                    {
                        self.saved_inventories.insert(name, inv);
                    }
                    pending_inv_sends.remove(id);
                    // Tell every still-connected peer to forget
                    // the departed player's cube.
                    let msg = PlayerRemoveMsg { id: *id };
                    if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                        server.broadcast_bytes("vx.player_remove", buf);
                    }
                    // Public departure notice. Hits the host's
                    // local log via chat_inbox + broadcasts to
                    // every still-connected peer.
                    let line = format!("* {depart_name} left.");
                    chat_inbox.push(line.clone());
                    if let Ok(buf) = bincode::serde::encode_to_vec(&line, cfg) {
                        server.broadcast_bytes("vx.chat", buf);
                    }
                }
                if !new_ids.is_empty() && !self.edit_history.is_empty() {
                    tracing::info!(
                        joined = ?new_ids,
                        edits = self.edit_history.len(),
                        "rebroadcasting edit history for new client(s)"
                    );
                    for e in &self.edit_history {
                        if let Ok(buf) = bincode::serde::encode_to_vec(e, cfg) {
                            server.broadcast_bytes("vx.edit", buf);
                        }
                    }
                }
                // Seed inventories for any newly-joined clients +
                // queue them an initial snapshot so their client UI
                // mirrors the authoritative server state. Restore
                // from the persisted side map when the client's
                // name matches a previous session.
                for id in &new_ids {
                    if !inventories.contains_key(id) {
                        let name = server.client_name(*id);
                        let inv = name
                            .as_ref()
                            .and_then(|n| self.saved_inventories.remove(n))
                            .unwrap_or_else(ClientInv::starter);
                        inventories.insert(*id, inv);
                    }
                    pending_inv_sends.insert(*id);
                    // Public arrival notice. Same path as the
                    // depart line above; the server's broadcast
                    // pushes to every peer including the new
                    // joiner, so they immediately see who else
                    // is around.
                    let name = server
                        .client_name(*id)
                        .unwrap_or_else(|| format!("p{id}"));
                    let line = format!("* {name} joined.");
                    chat_inbox.push(line.clone());
                    if let Ok(buf) = bincode::serde::encode_to_vec(&line, cfg) {
                        server.broadcast_bytes("vx.chat", buf);
                    }
                }
                // Roster broadcast: whenever the connected set
                // changes, ship the full id->name table to every
                // client so nameplates / Tab playerlist stay
                // current. Host owns id 0 with its own
                // `player_name`.
                // Send world seed to brand-new joiners so their
                // chunk workers generate matching terrain.
                if !new_ids.is_empty()
                    && let Ok(buf) =
                        bincode::serde::encode_to_vec(world_seed(), cfg)
                {
                    for id in &new_ids {
                        server.send_bytes_to_client(*id, "vx.seed", buf.clone());
                    }
                }
                // Ship current weather to new joiners too.
                if !new_ids.is_empty()
                    && let Ok(buf) =
                        bincode::serde::encode_to_vec(self.weather, cfg)
                {
                    for id in &new_ids {
                        server.send_bytes_to_client(
                            *id,
                            "vx.weather",
                            buf.clone(),
                        );
                    }
                }
                if !new_ids.is_empty() || !departed_ids.is_empty() {
                    let mut entries: Vec<(u32, String)> =
                        vec![(0, self.player_name.clone())];
                    for cid in server.client_ids() {
                        let name = server
                            .client_name(cid)
                            .unwrap_or_else(|| format!("p{cid}"));
                        entries.push((cid, name));
                    }
                    self.player_names.clear();
                    for (id, n) in &entries {
                        self.player_names.insert(*id, n.clone());
                    }
                    if let Ok(buf) = bincode::serde::encode_to_vec(&entries, cfg) {
                        server.broadcast_bytes("vx.names", buf);
                    }
                }
                self.known_client_ids = current_ids;
                // Validate every incoming edit against the sender's
                // inventory + spatial range before accepting it.
                //
                // - BREAK: sender's pose must be within REACH of the
                //   target voxel. Passing the inventory check is
                //   trivial (breaks never consume); the pickup
                //   credit happens here too.
                // - PLACE: sender must have the block AND be within
                //   REACH of the target voxel. Silent reject on
                //   either failure (the next `vx.inv` snapshot
                //   reconciles the predicted client state).
                const REACH: f32 = 6.5;
                for (cid, e) in raw_edits {
                    // Look up the sender's last reported pose; if
                    // we've never seen one, fall back to a sentinel
                    // far away so the range check rejects the edit.
                    let pose = poses.get(&cid).copied().unwrap_or(PlayerMsg {
                        id: cid,
                        pos: [1e9, 1e9, 1e9],
                        yaw: 0.0,
                        pitch: 0.0,
                    });
                    let voxel_center = [
                        e.x as f32 + 0.5,
                        e.y as f32 + 0.5,
                        e.z as f32 + 0.5,
                    ];
                    let dx = pose.pos[0] - voxel_center[0];
                    let dy = pose.pos[1] - voxel_center[1];
                    let dz = pose.pos[2] - voxel_center[2];
                    let dist2 = dx * dx + dy * dy + dz * dz;
                    if dist2 > REACH * REACH {
                        tracing::debug!(
                            client_id = cid,
                            block = e.block,
                            "edit rejected: out of reach"
                        );
                        // Tell the client what the voxel actually
                        // looks like server-side so its prediction
                        // gets reverted on the next tick.
                        let truth = self.world.get([e.x, e.y, e.z]);
                        let revert = EditMsg {
                            x: e.x,
                            y: e.y,
                            z: e.z,
                            block: truth.0,
                        };
                        if let Ok(buf) = bincode::serde::encode_to_vec(revert, cfg) {
                            server.send_bytes_to_client(cid, "vx.edit", buf);
                        }
                        pending_inv_sends.insert(cid);
                        continue;
                    }
                    if e.block == 0 {
                        // BREAK - capture old block to credit
                        // pickup, then apply.
                        let voxel = [e.x, e.y, e.z];
                        let broken = self.world.get(voxel);
                        if broken != BlockId::AIR
                            && let Some(inv) = inventories.get_mut(&cid)
                            && inv_pickup(&mut inv.inventory, broken)
                        {
                            pending_inv_sends.insert(cid);
                        }
                        to_apply.push(e);
                    } else {
                        // PLACE - require + consume from sender's
                        // inventory. Silently drop the placement if
                        // they don't have it (anti-cheat / desync).
                        let block = BlockId(e.block);
                        let ok = inventories
                            .get_mut(&cid)
                            .is_some_and(|inv| inv_consume(&mut inv.inventory, block));
                        if ok {
                            pending_inv_sends.insert(cid);
                            to_apply.push(e);
                        } else {
                            // Reverted: send the voxel's true state
                            // so the client's predicted block
                            // vanishes; also push the snapshot to
                            // restore the (unchanged) inventory.
                            let truth = self.world.get([e.x, e.y, e.z]);
                            let revert = EditMsg {
                                x: e.x,
                                y: e.y,
                                z: e.z,
                                block: truth.0,
                            };
                            if let Ok(buf) = bincode::serde::encode_to_vec(revert, cfg) {
                                server.send_bytes_to_client(cid, "vx.edit", buf);
                            }
                            pending_inv_sends.insert(cid);
                        }
                    }
                }
                // Apply incoming drag&drop intents on the server's
                // authoritative inventory.
                for (cid, act) in raw_drags {
                    if let Some(inv) = inventories.get_mut(&cid) {
                        inv_apply_drag(inv, act.from.into(), act.to.into(), act.split);
                        pending_inv_sends.insert(cid);
                    }
                }
                // Server also records every received edit so future
                // joiners get a complete history.
                for e in &to_apply {
                    self.edit_history.push(*e);
                }
                // Broadcast each player's pose (one packet each;
                // future: batch). Also rebroadcast any incoming
                // edits we'll apply below.
                for p in poses.values() {
                    if let Ok(buf) = bincode::serde::encode_to_vec(p, cfg) {
                        server.broadcast_bytes("vx.player", buf);
                    }
                }
                for e in &to_apply {
                    if let Ok(buf) = bincode::serde::encode_to_vec(e, cfg) {
                        server.broadcast_bytes("vx.edit", buf);
                    }
                }
                // Validate the deferred shovel-path requests
                // here while `poses` is still in scope; the
                // filtered voxel list is consumed by the
                // post-arm world-mutation pass.
                for (cid, v) in path_requests.iter() {
                    let pos = poses
                        .get(cid)
                        .copied()
                        .map(|p| p.pos)
                        .unwrap_or([1e9, 1e9, 1e9]);
                    let dx = pos[0] - v[0] as f32 - 0.5;
                    let dy = pos[1] - v[1] as f32 - 0.5;
                    let dz = pos[2] - v[2] as f32 - 0.5;
                    if dx * dx + dy * dy + dz * dz <= 49.0 {
                        path_apply.push(*v);
                    }
                }
                // Push authoritative inventory snapshots to every
                // client whose state changed this tick.
                for cid in pending_inv_sends.drain() {
                    if let Some(inv) = inventories.get(&cid) {
                        let snap = inv.to_snapshot();
                        if let Ok(buf) = bincode::serde::encode_to_vec(&snap, cfg) {
                            server.send_bytes_to_client(cid, "vx.inv", buf);
                        }
                    }
                }
                // Authoritative day_time broadcast on a slow
                // cadence (~once per second). Clients update
                // their local clock so the sun stays in sync
                // even if /day or /night was used to skip ahead.
                let now = std::time::Instant::now();
                let due = self
                    .last_day_broadcast
                    .map(|t| now.duration_since(t).as_secs_f32() >= 1.0)
                    .unwrap_or(true);
                if due
                    && let Ok(buf) =
                        bincode::serde::encode_to_vec(self.day_time, cfg)
                {
                    server.broadcast_bytes("vx.day", buf);
                    self.last_day_broadcast = Some(now);
                }
            }
            NetRole::Client {
                client,
                remote,
                pending_edits,
                pending_inv_actions,
            } => {
                client.poll();
                let my_id = client.client_id;
                {
                    let mut inbox = net::ClientInboxCtx {
                        to_apply: &mut to_apply,
                        client_snapshot: &mut client_snapshot,
                        mob_updates: &mut mob_updates,
                        mob_events: &mut mob_events,
                        mob_removals: &mut mob_removals,
                        player_removals: &mut player_removals,
                        interp_updates: &mut interp_updates,
                        pvp_self_damage: &mut pvp_self_damage,
                        pvp_self_heal: &mut pvp_self_heal,
                        chat_inbox: &mut chat_inbox,
                        names_update: &mut names_update,
                        teleport_request: &mut teleport_request,
                        day_time_update: &mut day_time_update,
                        seed_update: &mut seed_update,
                        weather_update: &mut weather_update,
                        snow_updates: &mut snow_updates,
                        liquid_level_updates: &mut liquid_level_updates,
                        drop_spawns: &mut drop_spawns,
                        drop_removes: &mut drop_removes,
                        torch_orient_updates: &mut torch_orient_updates,
                        kick_reason: &mut kick_reason,
                    };
                    net::process_client_inbox(
                        client,
                        remote,
                        pending_edits,
                        pending_inv_actions,
                        &mut inbox,
                    );
                }
                // Re-send pending edits / inventory drags on a
                // throttled schedule. Server-applied edits are
                // idempotent (set+broadcast), so duplicate arrivals
                // are correctness-safe, but we don't need them every
                // single tick - `RETRY_PERIOD` gates the resend.
                if do_retry {
                    for e in pending_edits.iter() {
                        if let Ok(buf) = bincode::serde::encode_to_vec(e, cfg) {
                            let _ = client.send_bytes("vx.edit", buf);
                        }
                    }
                    for a in pending_inv_actions.iter() {
                        if let Ok(buf) = bincode::serde::encode_to_vec(a, cfg) {
                            let _ = client.send_bytes("vx.inv_act", buf);
                        }
                    }
                }
                // Send own pose upstream.
                let me = PlayerMsg {
                    id: my_id.unwrap_or(0),
                    pos: [player_pos.x, player_pos.y, player_pos.z],
                    yaw: player_yaw,
                    pitch: player_pitch,
                };
                if let Ok(buf) = bincode::serde::encode_to_vec(me, cfg) {
                    let _ = client.send_bytes("vx.player", buf);
                }
            }
        }
        // Apply collected edits after the borrow on `self.net`
        // releases - both paths funnel through here so the
        // chunk-rebuild logic only lives in one place.
        for e in to_apply {
            let voxel = [e.x, e.y, e.z];
            self.world_set_tracked(voxel, BlockId(e.block));
            let coords = Self::affected_chunks(voxel);
            self.rebuild_chunks(&coords);
        }
        // Apply validated shovel-path requests (server side):
        // grass voxels become dirt + broadcast as vx.edit.
        if !path_apply.is_empty() {
            let cfg = bincode::config::standard();
            for v in path_apply.into_iter() {
                if self.world.get(v).0 != 3 {
                    continue;
                }
                self.world_set_tracked(v, BlockId(2));
                let coords = Self::affected_chunks(v);
                self.rebuild_chunks(&coords);
                let edit = EditMsg {
                    x: v[0],
                    y: v[1],
                    z: v[2],
                    block: 2,
                };
                self.edit_history.push(edit);
                if let NetRole::Server { server, .. } = &mut self.net
                    && let Ok(buf) =
                        bincode::serde::encode_to_vec(edit, cfg)
                {
                    server.broadcast_bytes("vx.edit", buf);
                }
            }
        }
        // Authoritative inventory snapshot from the server: replace
        // the local view wholesale. Anything the client mispredicted
        // (drag intent the server rejected, pickup the server
        // assigned to someone else) reconciles on this single
        // assignment.
        if let Some(snap) = client_snapshot {
            // Diff vs the pre-snapshot inventory so server-driven
            // pickups generate toasts the same way local pickups
            // do. Counts blocks by id; any positive delta becomes
            // a `+N` toast.
            let mut pre: HashMap<u16, u32> = HashMap::new();
            for (b, c) in self.inventory.iter().flatten() {
                *pre.entry(b.0).or_insert(0) += *c as u32;
            }
            for (i, s) in snap.inventory.iter().enumerate() {
                if i < self.inventory.len() {
                    self.inventory[i] = s.map(|(b, c)| (BlockId(b), c));
                }
            }
            for (i, s) in snap.craft.iter().enumerate() {
                if i < self.craft_grid.len() {
                    self.craft_grid[i] = s.map(|(b, c)| (BlockId(b), c));
                }
            }
            let mut post: HashMap<u16, u32> = HashMap::new();
            for (b, c) in self.inventory.iter().flatten() {
                *post.entry(b.0).or_insert(0) += *c as u32;
            }
            for (id, after) in post {
                let before = pre.get(&id).copied().unwrap_or(0);
                if after > before {
                    let delta = (after - before).min(u8::MAX as u32) as u8;
                    let block = BlockId(id);
                    let now = std::time::Instant::now();
                    if let Some(last) = self.pickup_toasts.last_mut()
                        && last.1 == block
                        && now.duration_since(last.0).as_secs_f32() < 1.5
                    {
                        last.0 = now;
                        last.2 = last.2.saturating_add(delta);
                    } else {
                        self.pickup_toasts.push((now, block, delta));
                    }
                }
            }
            if self.pickup_toasts.len() > 16 {
                let drop = self.pickup_toasts.len() - 16;
                self.pickup_toasts.drain(0..drop);
            }
        }
        for m in mob_updates {
            self.remote_mobs.insert(m.id, m);
        }
        // Fold incoming pose updates into the interpolation buffer
        // so the render path can lerp toward each new snapshot
        // instead of teleporting.
        for p in interp_updates {
            self.remote_interp
                .entry(p.id)
                .and_modify(|r| r.push(p))
                .or_insert_with(|| RemoteInterp::new(p));
        }
        // Drain chat lines collected while the `net` borrow was
        // held in the receive match. They render in the HUD chat
        // log with a 10s fade.
        if !chat_inbox.is_empty() {
            let now = std::time::Instant::now();
            for line in chat_inbox {
                // Mute filter: drop lines whose `<sender>` tag
                // or `* sender` prefix matches a muted name.
                if !self.muted_players.is_empty()
                    && line_is_muted(&line, &self.muted_players)
                {
                    continue;
                }
                self.chat_log.push((now, line));
            }
            // Cap log length so very long sessions don't grow
            // unbounded.
            if self.chat_log.len() > CHAT_LOG_MAX {
                let drop = self.chat_log.len() - CHAT_LOG_MAX;
                self.chat_log.drain(0..drop);
            }
        }
        // Apply the latest names roster (client-side; the server
        // already updates `player_names` inline when it builds the
        // broadcast).
        if let Some(entries) = names_update {
            self.player_names.clear();
            for (id, n) in entries {
                self.player_names.insert(id, n);
            }
        }
        if let Some(dt) = day_time_update {
            self.day_time = dt.rem_euclid(DAY_LENGTH_S);
        }
        if let Some(w) = weather_update {
            self.weather = w;
        }
        for (v, layers) in snow_updates {
            self.snow_positions.insert(v, layers.clamp(1, 8));
        }
        for (v, orient) in torch_orient_updates {
            self.torch_orient.insert(v, orient);
        }
        if !liquid_level_updates.is_empty() {
            for (v, level) in liquid_level_updates {
                self.liquid_level.insert(v, level.min(7));
            }
            // New levels reshape the slope mesh.
            self.liquid_mesh_dirty = true;
        }
        for m in drop_spawns {
            // Q.5: clients mirror the spawn but pickup is
            // host-authoritative, so the count + pickup-delay
            // here are decorative on the client side.
            self.dropped_items.push(DropItem {
                id: m.id,
                block: m.block,
                pos: m.pos,
                vel: [0.0, 2.0, 0.0],
                age: 0.0,
                count: 1,
                age_ticks: 0,
                pickup_delay_ticks: drops::PickupConfig::DEFAULT.pickup_delay_ticks,
            });
        }
        for id in drop_removes {
            self.dropped_items.retain(|d| d.id != id);
        }
        if let Some(reason) = kick_reason {
            // Server kicked us. Notify the player + bounce back
            // to the main menu cleanly. notify_server_leaving
            // would race the disconnect so we just drop the role.
            self.chat_log.push((
                std::time::Instant::now(),
                format!("Kicked by host: {reason}"),
            ));
            self.net = NetRole::Offline;
            self.state = AppState::MainMenu;
            self.browser_error = Some(format!("kicked: {reason}"));
        }
        if let Some(seed) = seed_update
            && seed != world_seed()
        {
            // Server picked a new world seed: adopt it and drop
            // every generated chunk so the next streaming pass
            // re-runs the workers with the new noise. We can't
            // mid-flight remap existing chunks since heights /
            // ore positions / biomes all changed.
            set_world_seed(seed);
            let to_unload: Vec<ChunkCoord> = self
                .world
                .iter_chunks()
                .map(|(c, _)| *c)
                .collect();
            if !to_unload.is_empty() {
                let renderer = self
                    .renderer
                    .as_mut()
                    .expect("renderer ready");
                for entry in self.chunk_meshes.drain(..) {
                    renderer.remove_mesh(entry.2);
                }
                for cc in &to_unload {
                    self.world.remove_chunk(*cc);
                }
            }
            self.pending_chunks.clear();
            self.pending_set.clear();
        }
        if let Some(t) = teleport_request {
            self.player_pos = Vec3::new(t.pos[0], t.pos[1], t.pos[2]);
            self.player_vel = Vec3::ZERO;
            self.fall_start_y = None;
            self.chat_log.push((
                std::time::Instant::now(),
                "You were summoned.".to_string(),
            ));
        }
        if pvp_self_heal > 0.0 {
            self.player_health = (self.player_health + pvp_self_heal)
                .min(PLAYER_MAX_HEALTH);
        }
        if pvp_self_damage > 0.0 {
            self.apply_player_damage(pvp_self_damage);
        }
        // Drop departed players from the client-side remote table
        // so their cubes stop rendering as soon as the server
        // tells us they left.
        if !player_removals.is_empty()
            && let NetRole::Client { remote, .. } = &mut self.net
        {
            for id in &player_removals {
                remote.remove(id);
            }
        }
        // Apply mob hits requested by clients. Server validates
        // reach against the sender's last known pose before
        // routing into the authoritative damage path.
        for (cid, mob_id) in mob_hits_to_apply {
            let sender_pose = match &self.net {
                NetRole::Server { poses, .. } => poses.get(&cid).copied(),
                _ => None,
            };
            let mob_pos = self.remote_mobs.get(&mob_id).map(|m| m.pos);
            let ok = match (sender_pose, mob_pos) {
                (Some(p), Some(mp)) => {
                    let dx = p.pos[0] - mp[0];
                    let dy = p.pos[1] - mp[1];
                    let dz = p.pos[2] - mp[2];
                    dx * dx + dy * dy + dz * dz
                        <= (MOB_ATTACK_REACH + 1.0).powi(2)
                }
                _ => false,
            };
            if ok {
                self.damage_mob(mob_id, Some(cid));
            }
        }
        // Client-side: apply incoming mob events (visual flash +
        // death sound). The server doesn't run this branch since
        // it already played them when authoritative damage fired.
        for evt in mob_events {
            self.mob_hurt_flash.insert(evt.id, 0.30);
            // Death event: start the local fade timer so the
            // body tilts + fades for `MOB_DEATH_FADE` seconds.
            // The server emits `vx.mob_remove` after the same
            // delay, which will evict the entry from
            // `remote_mobs` at the right moment.
            if evt.kind == 1 {
                self.mob_dying.insert(evt.id, MOB_DEATH_FADE);
            }
            // Attack event: kick off the local lunge timer so the
            // attacker's body briefly pushes forward.
            if evt.kind == 2 {
                self.mob_lunge.insert(evt.id, MOB_LUNGE_TIME);
            }
            if let Some(mob) = self.remote_mobs.get(&evt.id).copied() {
                let pos = mob.pos;
                let say_freq = if evt.kind == 1 { 200.0 } else { 280.0 };
                self.sfx_at("mob/sheep/say", pos, move |a, g| {
                    let _ = a.play_beep(say_freq, 0.16, 0.20 * g);
                });
            }
        }
        // Removals: clean up local mob bookkeeping mirroring what
        // damage_mob does server-side.
        for id in mob_removals {
            self.remote_mobs.remove(&id);
            self.mob_hurt_flash.remove(&id);
            self.mob_dying.remove(&id);
            self.mob_step_timers.remove(&id);
            self.mob_say_timers.remove(&id);
            self.mob_last_xz.remove(&id);
        }
        // Drop interpolation entries for any player who left.
        for id in &player_removals {
            self.remote_interp.remove(id);
        }
        // Periodic state flush. Server hosts AND offline
        // singleplayer both own the world state, so both auto-
        // save - a fresh-install user who plays offline and
        // quits without `/save` previously lost every build on
        // restart. The cadence matches the design doc's 60 s
        // autosave window.
        if matches!(self.net, NetRole::Server { .. } | NetRole::Offline)
            && self
                .last_state_save
                .is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(60))
        {
            self.save_server_state();
        }
    }

    fn new() -> Self {
        let mut registry = BlockRegistry::new();
        // Drive the runtime block registry from the BLOCK_TYPES
        // table so adding a new block kind is a one-place change.
        for bt in BLOCK_TYPES {
            let def = if bt.transparent {
                BlockDef::transparent(bt.name, bt.color)
            } else {
                BlockDef::solid(bt.name, bt.color)
            };
            registry.insert(BlockId(bt.id), def);
        }
        Self {
            state: AppState::MainMenu,
            settings: Settings::default(),
            requested_quit: false,
            app_start: std::time::Instant::now(),
            renderer: None,
            ui: None,
            registry,
            world: World::new(),
            current_dim: dim::OVERWORLD.to_string(),
            base_seed: 0,
            worlds: HashMap::new(),
            entities_by_dim: HashMap::new(),
            dim_worldgens: HashMap::new(),
            portal_in_ticks: 0,
            portal_cooldown_ticks: 0,
            chunk_meshes: Vec::new(),
            pending_chunks: VecDeque::new(),
            pending_set: HashSet::new(),
            workers: None,
            target: None,
            mine_target: None,
            player_pos: Vec3::new(0.0, 32.0, 0.0),
            player_vel: Vec3::ZERO,
            player_yaw: 0.0,
            player_pitch: -0.4,
            has_focus: true,
            last_mouse: None,
            mouse_dx: 0.0,
            mouse_dy: 0.0,
            esc_prev: false,
            e_prev: false,
            f_prev: false,
            hud_visible: true,
            f1_prev: false,
            chat_open: false,
            chat_input: String::new(),
            chat_log: Vec::new(),
            chat_history: Vec::new(),
            chat_history_idx: None,
            t_prev: false,
            lmb_pending: false,
            rmb_pending: false,
            lmb_held: false,
            rmb_held: false,
            mining: None,
            player_health: PLAYER_MAX_HEALTH,
            fall_start_y: None,
            hurt_flash: 0.0,
            regen_accum: 0.0,
            kill_count: 0,
            blocks_broken: 0,
            blocks_placed: 0,
            distance_walked: 0.0,
            last_walk_xz: None,
            place_history: Vec::new(),
            z_prev: false,
            show_chunk_grid: false,
            g_prev: false,
            highlight_mode: HighlightMode::Auto,
            h_prev: false,
            fov_y_rad: 60_f32.to_radians(),
            sprinting: false,
            stamina: STAMINA_MAX,
            player_air: AIR_MAX_SEC,
            remote_interp: HashMap::new(),
            player_name: {
                // Pseudo-random 4-digit suffix so repeat-launch
                // sessions don't accidentally collide.
                let n = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos())
                    .unwrap_or(0)
                    % 10000;
                format!("Player{n:04}")
            },
            player_names: HashMap::new(),
            tab_held: false,
            pickup_toasts: Vec::new(),
            inventory: {
                // Curated starter hotbar: slot 0 left empty so the
                // player has a free hand for picked-up items; slots
                // 1..8 hold the four wood tools + four basic block
                // bundles so the player can immediately mine across
                // every category without crafting first.
                let mut inv: [Option<(BlockId, u8)>; INVENTORY_SLOTS] =
                    [None; INVENTORY_SLOTS];
                let seed: &[(u16, u8)] = &[
                    (16, 1), // wood pickaxe
                    (23, 1), // wood sword
                    (29, 1), // wood shovel
                    (32, 1), // wood axe
                    (2, 16), // dirt
                    (7, 16), // planks
                    (8, 16), // cobble
                    (10, 4), // torch
                ];
                for (i, (bid, count)) in seed.iter().enumerate() {
                    let slot = i + 1;
                    if slot < HOTBAR_SLOTS {
                        inv[slot] = Some((BlockId(*bid), *count));
                    }
                }
                inv
            },
            selected_slot: 0,
            inventory_open: false,
            mp_menu_open: false,
            craft_grid: [None; 4],
            wheel_accum: 0.0,
            grounded: false,
            viewport: (1, 1),
            fps_ema: 60.0,
            // Start near mid-morning so the demo opens on a bright
            // sky rather than at sunrise.
            day_time: DAY_LENGTH_S * 0.3,
            net: NetRole::Offline,
            player_cube: None,
            liquid_mesh_handles: HashMap::new(),
            last_built_worldgen: None,
            dropped_items: Vec::new(),
            next_drop_id: 1,
            flow_sfx_accum: 0.0,
            liquid_mesh_dirty: true,
            sheep_mesh: None,
            sheep_head_mesh: None,
            voxel_atlas: None,
            atlas_rgba: None,
            egui_atlas: None,
            texture_dir: None,
            particles: {
                // Particle system starts with one "ambient" emitter
                // (rate=0, max=0) that only contributes its gravity
                // + interpolation curves to direct-burst particles.
                let mut s = infengine_particles::ParticleSystem::new();
                s.add_emitter(infengine_particles::Emitter {
                    rate: 0.0,
                    max_particles: 0,
                    gravity: [0.0, -10.0, 0.0],
                    color_start: [0.75, 0.55, 0.30, 1.0],
                    color_end: [0.50, 0.32, 0.18, 0.0],
                    size_start: 0.12,
                    size_end: 0.04,
                    lifetime: 0.5,
                    ..Default::default()
                });
                s
            },
            edit_history: Vec::new(),
            known_client_ids: HashSet::new(),
            last_state_save: None,
            saved_inventories: HashMap::new(),
            // Default slug for the cold-start path. The legacy
            // migration step overrides this to "legacy" if a pre-A4
            // save is found; the Worlds picker rewrites it to the
            // user's pick on New / Load.
            current_world_slug: Some(world_save::DEFAULT_SLUG.to_string()),
            net_tick: 0,
            audio: None,
            audio_bank: infengine_audio::AudioBank::new(),
            sounds_dir: None,
            step_cooldown: 0.0,
            heartbeat_accum: 0.0,
            cactus_damage_accum: 0.0,
            fly_mode: false,
            god_mode: false,
            invuln_remaining: 0.0,
            was_night: false,
            was_in_water: false,
            world_locked: false,
            mob_cap: MOB_CAP_DEFAULT,
            freeze_time: false,
            weather: 0,
            lightning_flash: 0.0,
            snow_accum: 0.0,
            snow_rate: 0.4,
            chicken_lay_accum: 0.0,
            debug_chicken_feathers: false,
            drop_tick_accum: 0.0,
            next_lightning_at: 6.0,
            muted_players: HashSet::new(),
            head_bob_phase: 0.0,
            camera_mode: 0,
            f5_prev: false,
            show_debug_panel: false,
            f6_prev: false,
            // Pathfinding enabled by default - matches MC's
            // `FlowingFluid.getSpread` which always biases toward
            // the closest hole. Toggle off via F6 for radial
            // spread (useful when debugging spread bugs).
            liquid_bias_drops: true,
            f4_prev: false,
            q_prev: false,
            home_pos: None,
            spawn_pos: None,
            session_started: None,
            last_day_broadcast: None,
            show_debug: false,
            f3_prev: false,
            torch_positions: HashSet::new(),
            torch_orient: HashMap::new(),
            snow_positions: HashMap::new(),
            chests: HashMap::new(),
            chest_open: None,
            chest_held: None,
            furnaces: HashMap::new(),
            furnace_runtimes: HashMap::new(),
            fires: HashMap::new(),
            furnace_open: None,
            furnace_held: None,
            liquid_schedule: LiquidSchedule::new(),
            liquid_clock: 0,
            liquid_level: HashMap::new(),
            liquid_positions: HashSet::new(),
            mobs: Vec::new(),
            remote_mobs: HashMap::new(),
            last_mob_spawn_time: 0.0,
            next_mob_id: 1,
            mob_step_timers: HashMap::new(),
            mob_say_timers: HashMap::new(),
            mob_last_xz: HashMap::new(),
            mob_hurt_flash: HashMap::new(),
            mob_no_hit_time: HashMap::new(),
            mob_dying: HashMap::new(),
            mob_lunge: HashMap::new(),
            master_addr: format!(
                "91.232.214.139:{}",
                infengine_net::master::MASTER_PORT
            ),
            direct_addr_buf: "127.0.0.1:7777".into(),
            host_name_buf: "My InfEngine Server".into(),
            host_port_buf: "7777".into(),
            host_announce: true,
            browser_servers: Vec::new(),
            browser_error: None,
            browser_last_refresh: None,
            browser_query_rx: None,
            announce_query_rx: None,
            master_token: None,
            last_heartbeat: None,
            new_world_name_buf: String::new(),
        }
    }

    /// F.2d: build a fresh `WorldgenCtx` snapshot from the live
    /// per-dim runtime state. Captured on the main thread, then
    /// shipped to the worker pool so the generator runs with the
    /// EXACT base seed + dim + profile the player saw at enqueue
    /// time. A later `switch_dim` between enqueue + pickup no
    /// longer poisons the gen.
    fn current_worldgen_ctx(&self) -> worldgen::WorldgenCtx {
        let profile = self
            .dim_worldgens
            .get(&self.current_dim)
            .copied()
            .unwrap_or_else(WorldGen::current);
        worldgen::WorldgenCtx {
            base_seed: self.base_seed,
            dim: self.current_dim.clone(),
            profile,
        }
    }

    /// Phase F: swap the active dim to `target`. Parks the
    /// current dim's voxel + entity state in the `_by_dim` maps
    /// and pulls the target dim's state back out (minting empty
    /// if missing).
    ///
    /// F.2e: NO LONGER writes the global `WORLD_SEED` /
    /// `WORLDGEN` atomics. Chunk gen now goes through
    /// `current_worldgen_ctx()` -> `generate_chunk_at_ctx()` so
    /// the per-dim seed + profile are passed by value into every
    /// worker job, race-free against any other thread or future
    /// dim swap.
    ///
    /// Idempotent if `target == current_dim`. The player's pose
    /// and inventory are untouched - they carry across dims.
    /// Mesh handles are NOT rebuilt here; the caller is
    /// responsible for `rebuild_all_meshes` (or letting the
    /// streaming code repopulate on the next frame).
    #[allow(dead_code)]
    fn switch_dim(&mut self, target: &str) {
        if target == self.current_dim {
            return;
        }
        // Park the current dim's state in the parking maps.
        // Move-semantics on `std::mem::take` zeroes the live
        // fields; we'll fill them back in from the target dim.
        let parked = dim::DimWorld {
            world: std::mem::replace(&mut self.world, World::new()),
            chests: std::mem::take(&mut self.chests),
            furnaces: std::mem::take(&mut self.furnaces),
            furnace_runtimes: std::mem::take(&mut self.furnace_runtimes),
            torch_positions: std::mem::take(&mut self.torch_positions),
            torch_orient: std::mem::take(&mut self.torch_orient),
            snow_positions: std::mem::take(&mut self.snow_positions),
            fires: std::mem::take(&mut self.fires),
            liquid_schedule: std::mem::replace(
                &mut self.liquid_schedule,
                LiquidSchedule::new(),
            ),
            liquid_clock: std::mem::take(&mut self.liquid_clock),
            liquid_level: std::mem::take(&mut self.liquid_level),
            liquid_positions: std::mem::take(&mut self.liquid_positions),
        };
        self.worlds.insert(self.current_dim.clone(), parked);
        let parked_ents = dim::DimEntities {
            mobs: std::mem::take(&mut self.mobs),
            drops: std::mem::take(&mut self.dropped_items),
            next_mob_id: self.next_mob_id,
            next_drop_id: self.next_drop_id,
        };
        self.entities_by_dim
            .insert(self.current_dim.clone(), parked_ents);

        // Pull the target dim's state out (mint empty if first
        // visit this session).
        let target_world = self
            .worlds
            .remove(target)
            .unwrap_or_default();
        self.world = target_world.world;
        self.chests = target_world.chests;
        self.furnaces = target_world.furnaces;
        self.furnace_runtimes = target_world.furnace_runtimes;
        self.torch_positions = target_world.torch_positions;
        self.torch_orient = target_world.torch_orient;
        self.snow_positions = target_world.snow_positions;
        self.fires = target_world.fires;
        self.liquid_schedule = target_world.liquid_schedule;
        self.liquid_clock = target_world.liquid_clock;
        self.liquid_level = target_world.liquid_level;
        self.liquid_positions = target_world.liquid_positions;
        let target_ents = self
            .entities_by_dim
            .remove(target)
            .unwrap_or_else(dim::DimEntities::empty);
        self.mobs = target_ents.mobs;
        self.dropped_items = target_ents.drops;
        self.next_mob_id = target_ents.next_mob_id;
        self.next_drop_id = target_ents.next_drop_id;

        // F.2e: switch_dim no longer touches the worldgen
        // atomics. Chunk gen reads the per-dim ctx via
        // `current_worldgen_ctx()`, which derives directly from
        // `current_dim` + `base_seed` + `dim_worldgens[target]`.
        // The atomics survive only as a "last-used" cache for UI
        // (HUD biome label, /biome chat command, the picker
        // dropdown); leaving them alone here means a worker job
        // already in-flight with a captured ctx finishes against
        // the dim it was enqueued against, not the one we just
        // swapped to.
        self.current_dim = target.to_string();
        // Drop renderer mesh handles for the chunks we just
        // parked - the active dim's chunk list completely
        // changed.
        if let Some(renderer) = self.renderer.as_mut() {
            for entry in self.chunk_meshes.drain(..) {
                renderer.remove_mesh(entry.2);
            }
            for (_, h) in self.liquid_mesh_handles.drain() {
                renderer.remove_mesh(h);
            }
        }
        // Force the streaming pipeline to forget what was
        // pending for the previous dim - those coords mean
        // something different now.
        self.pending_chunks.clear();
        self.pending_set.clear();
        // Open UIs reference position-keyed containers that
        // belong to the parked dim. Close them on switch so the
        // player doesn't see a stale chest panel.
        self.chest_open = None;
        self.chest_held = None;
        self.furnace_open = None;
        self.furnace_held = None;
        self.liquid_mesh_dirty = true;
    }

    /// Phase F.6: increment / reset the in-portal counter +
    /// fire a cross-dim teleport when the threshold is reached.
    /// Called once per frame from the main tick loop.
    ///
    /// The check uses the player's foot voxel (rounded floor of
    /// player_pos.x/z; player_pos.y - PLAYER_HEIGHT/2). When
    /// the foot voxel is a `PORTAL_ID` and we're not on
    /// cool-down, increment; on miss, reset to 0.
    fn tick_player_portal(&mut self) {
        if self.portal_cooldown_ticks > 0 {
            self.portal_cooldown_ticks -= 1;
            self.portal_in_ticks = 0;
            return;
        }
        // Foot voxel: the cell the player's feet sit in. Round
        // X/Z to floor; Y is the integer floor of the player's
        // base.
        let foot = [
            self.player_pos.x.floor() as i32,
            (self.player_pos.y - PLAYER_HEIGHT * 0.5).floor() as i32,
            self.player_pos.z.floor() as i32,
        ];
        let in_portal = self.world.get(foot).0 == block::PORTAL_ID
            || self.world.get([foot[0], foot[1] + 1, foot[2]]).0
                == block::PORTAL_ID;
        if !in_portal {
            self.portal_in_ticks = 0;
            return;
        }
        self.portal_in_ticks = self.portal_in_ticks.saturating_add(1);
        if self.portal_in_ticks >= block::portal::PORTAL_TRIGGER_TICKS {
            self.do_cross_dim_teleport(foot);
        }
    }

    /// Phase F.6: fire a cross-dim teleport. Compute the
    /// destination coord (scaled by 8), switch_dim into the
    /// paired dim, find / build a return portal, place the
    /// player adjacent, and set the cool-down so they don't
    /// instantly bounce back.
    fn do_cross_dim_teleport(&mut self, from_pos: [i32; 3]) {
        let target_dim = block::portal::pair_dim(&self.current_dim);
        let dest = block::portal::pair_coord(&self.current_dim, from_pos);
        // Swap the active dim. switch_dim does the heavy lifting
        // (move chunks/entities to the parking maps + load the
        // target dim's state).
        self.switch_dim(target_dim);
        // Find or build a return portal at the scaled coord.
        let cfg = bincode::config::standard();
        let mut edits: Vec<[i32; 3]> = Vec::new();
        let spawn = block::portal::ensure_return_portal(
            &mut self.world,
            dest,
            Some(&mut edits),
        );
        // Broadcast portal edits so remote peers see the new
        // frame appear. Guarded on the renderer being live so
        // headless / test callers don't hit the rebuild path.
        if !edits.is_empty() {
            if self.renderer.is_some() {
                let coords = Self::affected_chunks(dest);
                self.rebuild_chunks(&coords);
            }
            let portal_id = block::PORTAL_ID;
            let obsidian_id = block::OBSIDIAN_ID;
            let edits_to_send: Vec<EditMsg> = edits
                .iter()
                .map(|p| EditMsg {
                    x: p[0],
                    y: p[1],
                    z: p[2],
                    block: {
                        let v = self.world.get(*p).0;
                        if v == portal_id || v == obsidian_id { v } else { 0 }
                    },
                })
                .collect();
            match &mut self.net {
                NetRole::Offline => {}
                NetRole::Server { server, .. } => {
                    for e in &edits_to_send {
                        self.edit_history.push(*e);
                        if let Ok(buf) =
                            bincode::serde::encode_to_vec(*e, cfg)
                        {
                            server.broadcast_bytes("vx.edit", buf);
                        }
                    }
                }
                NetRole::Client { pending_edits, .. } => {
                    for e in &edits_to_send {
                        pending_edits.push(*e);
                    }
                }
            }
        }
        // Park the player adjacent to the return portal.
        self.player_pos = glam::Vec3::new(
            spawn[0] as f32 + 0.5,
            spawn[1] as f32,
            spawn[2] as f32 + 0.5,
        );
        self.player_vel = glam::Vec3::ZERO;
        self.portal_in_ticks = 0;
        self.portal_cooldown_ticks = block::portal::PORTAL_COOLDOWN_TICKS;
        self.chat_log.push((
            std::time::Instant::now(),
            format!("-> dim '{}'", target_dim),
        ));
    }

    /// Drop every loaded chunk + the per-chunk mesh handles +
    /// tracker sets that the regenerated world will repopulate.
    /// Used by `Play offline` when the menu picks a different
    /// `WorldGen` variant, so the new world isn't a mix of the
    /// old + new chunk layouts.
    fn reset_world(&mut self) {
        let coords: Vec<ChunkCoord> = self
            .world
            .iter_chunks()
            .map(|(c, _)| *c)
            .collect();
        if let Some(renderer) = self.renderer.as_mut() {
            for entry in self.chunk_meshes.drain(..) {
                renderer.remove_mesh(entry.2);
            }
            for (_, h) in self.liquid_mesh_handles.drain() {
                renderer.remove_mesh(h);
            }
        }
        for cc in coords {
            self.world.remove_chunk(cc);
        }
        self.pending_chunks.clear();
        self.pending_set.clear();
        self.snow_positions.clear();
        self.torch_positions.clear();
        self.torch_orient.clear();
        self.liquid_schedule.clear();
        self.liquid_clock = 0;
        self.liquid_level.clear();
        self.liquid_positions.clear();
        self.liquid_mesh_dirty = true;
        self.chests.clear();
        self.chest_open = None;
        self.chest_held = None;
        self.furnaces.clear();
        self.furnace_runtimes.clear();
        self.furnace_open = None;
        self.furnace_held = None;
    }

    /// Close the chest UI. Drops the cursor-held stack back into
    /// the chest at the first free slot (or onto the floor as a
    /// drop if the chest is gone / full). Called by ESC, the
    /// window's close button, and the periodic out-of-range check.
    fn close_chest_ui(&mut self) {
        let pos = self.chest_open.take();
        let held = self.chest_held.take();
        if let Some(p) = pos {
            let leftovers = block::chest::recover_held_on_close(
                self.chests.get_mut(&p),
                held,
            );
            for id in leftovers {
                self.spawn_drop(BlockId(id), p);
            }
        }
    }

    /// Close the furnace UI. Mirrors `close_chest_ui` but routes
    /// the held-stack recovery through `block::furnace`; the
    /// furnace's accept filters mean a held stack of the wrong
    /// type for input / fuel falls as floor drops.
    fn close_furnace_ui(&mut self) {
        let pos = self.furnace_open.take();
        let held = self.furnace_held.take();
        if let Some(p) = pos {
            let leftovers = block::furnace::recover_held_on_close(
                self.furnaces.get_mut(&p),
                held,
            );
            for id in leftovers {
                self.spawn_drop(BlockId(id), p);
            }
        }
    }

    /// Synchronously generate the initial chunk grid so the player
    /// drops into a non-empty world. Streaming workers take over once
    /// the player starts moving.
    ///
    /// F.2d: builds a `WorldgenCtx` snapshot once before the loop
    /// and passes it through `generate_chunk_at_ctx`. Pure-fn path,
    /// no atomic reads.
    fn build_world(&mut self) {
        let ctx = self.current_worldgen_ctx();
        for cz in -INITIAL_CHUNKS..=INITIAL_CHUNKS {
            for cx in -INITIAL_CHUNKS..=INITIAL_CHUNKS {
                for cy in 0..VERTICAL_CHUNKS {
                    let cc: ChunkCoord = [cx, cy, cz];
                    let chunk = worldgen::generate_chunk_at_ctx(cc, &ctx);
                    let origin = World::chunk_world_origin(cc);
                    let cs = CHUNK_SIZE as i32;
                    for ly in 0..cs {
                        for lz in 0..cs {
                            for lx in 0..cs {
                                let b = chunk.get(
                                    lx as usize,
                                    ly as usize,
                                    lz as usize,
                                );
                                let v = [
                                    origin.x as i32 + lx,
                                    origin.y as i32 + ly,
                                    origin.z as i32 + lz,
                                ];
                                if b.0 == 15 {
                                    self.snow_positions
                                        .entry(v)
                                        .or_insert(1);
                                } else if b.0 == TORCH_ID
                                    || b.0 == GLOWSTONE_ID
                                {
                                    self.torch_positions.insert(v);
                                } else if let Some(lq) = liquid_for(b) {
                                    self.liquid_schedule.schedule(
                                        v,
                                        self.liquid_clock
                                            + lq.tick_period.max(1) as u64,
                                    );
                                    if self.liquid_positions.insert(v) {
                                        self.liquid_mesh_dirty = true;
                                    }
                                }
                            }
                        }
                    }
                    self.world.insert_chunk(cc, chunk);
                }
            }
        }
        self.rebuild_all_meshes();
        self.last_built_worldgen = Some(WorldGen::current());
        info!(
            chunks = self.world.iter_chunks().count(),
            submeshes = self.chunk_meshes.len(),
            worldgen = ?WorldGen::current(),
            "initial world built"
        );
    }

    /// Streaming step: queue missing chunks within `VIEW_DISTANCE`
    /// of the player, unload chunks beyond `UNLOAD_DISTANCE`, then
    /// process up to `CHUNKS_PER_FRAME` queued chunks this frame.
    fn stream_chunks(&mut self) {
        let cs = CHUNK_SIZE as i32;
        let player_chunk = [
            (self.player_pos.x.floor() as i32).div_euclid(cs),
            0,
            (self.player_pos.z.floor() as i32).div_euclid(cs),
        ];
        let view_dist = self.settings.view_distance.clamp(2, 12) as i32;
        let unload_dist = view_dist + 2;

        // Queue missing chunks inside the view radius. Vertical
        // column always loaded (cy 0..VERTICAL_CHUNKS) so mountain
        // peaks + sky builds never pop in.
        for dz in -view_dist..=view_dist {
            for dx in -view_dist..=view_dist {
                for cy in 0..VERTICAL_CHUNKS {
                    let cc = [player_chunk[0] + dx, cy, player_chunk[2] + dz];
                    if self.world.chunk(cc).is_none() && self.pending_set.insert(cc) {
                        self.pending_chunks.push_back(cc);
                    }
                }
            }
        }

        // Unload chunks beyond the keep radius. Collect first to
        // avoid borrowing `self.world` immutably while mutating.
        let to_unload: Vec<ChunkCoord> = self
            .world
            .iter_chunks()
            .map(|(c, _)| *c)
            .filter(|c| {
                let dx = (c[0] - player_chunk[0]).abs();
                let dz = (c[2] - player_chunk[2]).abs();
                dx > unload_dist || dz > unload_dist
            })
            .collect();
        if !to_unload.is_empty() {
            let renderer = self.renderer.as_mut().expect("renderer ready");
            let mut keep = Vec::with_capacity(self.chunk_meshes.len());
            for entry in self.chunk_meshes.drain(..) {
                if to_unload.contains(&entry.0) {
                    renderer.remove_mesh(entry.2);
                } else {
                    keep.push(entry);
                }
            }
            self.chunk_meshes = keep;
            for cc in &to_unload {
                self.world.remove_chunk(*cc);
            }
        }

        // Dispatch pending chunks to the worker pool. The request
        // channel is unbounded; workers consume at their own rate.
        // F.2d: snapshot the current worldgen ctx on the main
        // thread and ship a copy with every job. If switch_dim
        // fires between enqueue and pickup, the worker still gens
        // against the captured ctx, not the stomped atomics.
        if let Some(pool) = self.workers.as_ref() {
            let ctx = self.current_worldgen_ctx();
            while let Some(cc) = self.pending_chunks.pop_front() {
                let job = ChunkJob {
                    coord: cc,
                    ctx: ctx.clone(),
                };
                if pool.req_tx.send(job).is_err() {
                    self.pending_chunks.push_front(cc);
                    break;
                }
            }
        }

        // Drain up to RESULTS_PER_FRAME finished chunks from the
        // workers. Each result: insert chunk + register meshes + remesh
        // the 4 horizontal neighbours so face culling at boundaries
        // stays correct.
        let mut delivered: Vec<WorkerResult> = Vec::new();
        if let Some(pool) = self.workers.as_ref() {
            while delivered.len() < RESULTS_PER_FRAME
                && let Ok(r) = pool.res_rx.try_recv()
            {
                delivered.push(r);
            }
        }
        for result in delivered {
            self.pending_set.remove(&result.coord);
            // The chunk might have been unloaded above (player walked
            // past it while the worker was busy). Drop stale data.
            let dx = (result.coord[0] - player_chunk[0]).abs();
            let dz = (result.coord[2] - player_chunk[2]).abs();
            if dx > unload_dist || dz > unload_dist {
                continue;
            }
            if self.world.chunk(result.coord).is_some() {
                continue;
            }
            // Scan the freshly-generated chunk for snow + torch
            // voxels so their special render paths pick them up
            // before the world_set_tracked writes ever fire.
            {
                let chunk_ref = &result.chunk;
                let origin = World::chunk_world_origin(result.coord);
                let cs = CHUNK_SIZE as i32;
                for ly in 0..cs {
                    for lz in 0..cs {
                        for lx in 0..cs {
                            let b = chunk_ref
                                .get(lx as usize, ly as usize, lz as usize);
                            if b.0 == 15 {
                                self.snow_positions
                                    .entry([
                                        origin.x as i32 + lx,
                                        origin.y as i32 + ly,
                                        origin.z as i32 + lz,
                                    ])
                                    .or_insert(1);
                            } else if b.0 == TORCH_ID
                                || b.0 == GLOWSTONE_ID
                            {
                                self.torch_positions.insert([
                                    origin.x as i32 + lx,
                                    origin.y as i32 + ly,
                                    origin.z as i32 + lz,
                                ]);
                            } else if let Some(lq) = liquid_for(b) {
                                let wv = [
                                    origin.x as i32 + lx,
                                    origin.y as i32 + ly,
                                    origin.z as i32 + lz,
                                ];
                                self.liquid_schedule.schedule(
                                    wv,
                                    self.liquid_clock
                                        + lq.tick_period.max(1) as u64,
                                );
                                if self.liquid_positions.insert(wv) {
                                    self.liquid_mesh_dirty = true;
                                }
                            }
                        }
                    }
                }
            }
            self.world.insert_chunk(result.coord, result.chunk);
            // Use the worker's submeshes directly for this chunk so
            // the parallel meshing work isn't thrown away. Boundary
            // faces between this chunk and an already-loaded
            // neighbour stay emitted (worker treated outside as air),
            // but they sit at the boundary plane between two solid
            // voxels and are never visible.
            let cc = result.coord;
            {
                let renderer = self.renderer.as_mut().expect("renderer ready");
                let atlas = self.voxel_atlas.clone();
                for sub in result.submeshes {
                    // Skip empty submeshes - registering a
                    // zero-vertex mesh would panic the shadow
                    // pass's `vertex_buffer.slice(..)`.
                    if sub.vertices.is_empty() || sub.indices.is_empty() {
                        continue;
                    }
                    // Skip blocks that own a dedicated render path
                    // (torch, snow, water - everything marked
                    // `F_DEDICATED_RENDER`). They get drawn by
                    // per-voxel slab / pillar passes downstream.
                    if block_skips_chunk_mesh(sub.block) {
                        continue;
                    }
                    let handle = renderer.create_mesh(
                        &format!("voxel_chunk_{}_{}_{}_b{}", cc[0], cc[1], cc[2], sub.block.0),
                        &sub.vertices,
                        &sub.indices,
                    );
                    if let Some(a) = atlas.clone() {
                        renderer.bind_mesh_albedo_arc(handle, a);
                    }
                    self.chunk_meshes.push((cc, sub.block, handle));
                }
            }
            // Re-mesh the 4 horizontal neighbours so their boundary
            // faces facing the freshly-inserted chunk get hidden.
            let neighbours = [
                [cc[0] - 1, cc[1], cc[2]],
                [cc[0] + 1, cc[1], cc[2]],
                [cc[0], cc[1], cc[2] - 1],
                [cc[0], cc[1], cc[2] + 1],
            ];
            self.rebuild_chunks(&neighbours);
        }
    }

    fn affected_chunks(world_voxel: [i32; 3]) -> Vec<ChunkCoord> {
        // The edited voxel always belongs to one chunk. If it sits on
        // a chunk boundary (local 0 or CHUNK_SIZE-1 on any axis) the
        // neighbour chunk's faces facing it become visible/hidden too,
        // so we rebuild that chunk as well.
        let cs = CHUNK_SIZE as i32;
        let cc = [
            world_voxel[0].div_euclid(cs),
            world_voxel[1].div_euclid(cs),
            world_voxel[2].div_euclid(cs),
        ];
        let lc = [
            world_voxel[0].rem_euclid(cs),
            world_voxel[1].rem_euclid(cs),
            world_voxel[2].rem_euclid(cs),
        ];
        let mut out = vec![cc];
        if lc[0] == 0 {
            out.push([cc[0] - 1, cc[1], cc[2]]);
        }
        if lc[0] == cs - 1 {
            out.push([cc[0] + 1, cc[1], cc[2]]);
        }
        if lc[1] == 0 {
            out.push([cc[0], cc[1] - 1, cc[2]]);
        }
        if lc[1] == cs - 1 {
            out.push([cc[0], cc[1] + 1, cc[2]]);
        }
        if lc[2] == 0 {
            out.push([cc[0], cc[1], cc[2] - 1]);
        }
        if lc[2] == cs - 1 {
            out.push([cc[0], cc[1], cc[2] + 1]);
        }
        out
    }

    fn rebuild_chunks(&mut self, coords: &[ChunkCoord]) {
        let renderer = self.renderer.as_mut().expect("renderer ready");
        // Release old mesh handles for the affected chunks so the
        // mesh registry doesn't accumulate stale buffers across edits.
        let mut keep = Vec::with_capacity(self.chunk_meshes.len());
        for entry in self.chunk_meshes.drain(..) {
            if coords.contains(&entry.0) {
                renderer.remove_mesh(entry.2);
            } else {
                keep.push(entry);
            }
        }
        self.chunk_meshes = keep;
        let atlas = self.voxel_atlas.clone();
        for coord in coords {
            let Some(chunk) = self.world.chunk(*coord) else {
                continue;
            };
            let origin = World::chunk_world_origin(*coord);
            for sub in chunk_mesh_with_atlas(chunk, origin, atlas_uv_rect, block_is_opaque) {
                if sub.vertices.is_empty() || sub.indices.is_empty() {
                    continue;
                }
                // Skip torch + snow submeshes: both render
                // through dedicated world-pass meshes (thin
                // torch + thin snow slab) instead of as full
                // cube voxels.
                if block_skips_chunk_mesh(sub.block) {
                    continue;
                }
                let handle = renderer.create_mesh(
                    &format!(
                        "voxel_chunk_{}_{}_{}_b{}",
                        coord[0], coord[1], coord[2], sub.block.0
                    ),
                    &sub.vertices,
                    &sub.indices,
                );
                if let Some(a) = atlas.clone() {
                    renderer.bind_mesh_albedo_arc(handle, a);
                }
                self.chunk_meshes.push((*coord, sub.block, handle));
            }
        }
    }

    /// Walk every chunk in the world and register its mesh handles.
    /// Used at startup. Edits use `rebuild_chunks` for the affected
    /// chunk + its solid neighbours so the cost stays local.
    fn rebuild_all_meshes(&mut self) {
        self.chunk_meshes.clear();
        let atlas = self.voxel_atlas.clone();
        let renderer = self.renderer.as_mut().expect("renderer ready");
        let coords: Vec<ChunkCoord> = self.world.iter_chunks().map(|(c, _)| *c).collect();
        for coord in coords {
            let chunk = self.world.chunk(coord).expect("chunk present");
            let origin = World::chunk_world_origin(coord);
            let subs = chunk_mesh_with_atlas(chunk, origin, atlas_uv_rect, block_is_opaque);
            for sub in subs {
                if sub.vertices.is_empty() || sub.indices.is_empty() {
                    continue;
                }
                // Skip torch + snow submeshes: both render
                // through dedicated world-pass meshes (thin
                // torch + thin snow slab) instead of as full
                // cube voxels.
                if block_skips_chunk_mesh(sub.block) {
                    continue;
                }
                let handle = renderer.create_mesh(
                    &format!(
                        "voxel_chunk_{}_{}_{}_b{}",
                        coord[0], coord[1], coord[2], sub.block.0
                    ),
                    &sub.vertices,
                    &sub.indices,
                );
                if let Some(a) = atlas.clone() {
                    renderer.bind_mesh_albedo_arc(handle, a);
                }
                self.chunk_meshes.push((coord, sub.block, handle));
            }
        }
    }

    /// Returns true while *any* in-game popup / modal is open.
    /// Central point used by the cursor + input + click gates so
    /// future popups only need to add one flag here instead of
    /// fanning out to every guard site.
    fn any_panel_open(&self) -> bool {
        self.inventory_open
            || self.mp_menu_open
            || self.chat_open
            || self.show_debug_panel
            || self.chest_open.is_some()
            || self.furnace_open.is_some()
    }

    fn apply_cursor_settings(&self, window: &EngineWindow) {
        use infengine_window::raw_winit::window::CursorGrabMode;
        let playing = matches!(self.state, AppState::Playing);
        let raw = window.raw();
        // Only grab when we have OS focus. Trying to grab an
        // unfocused window on Windows silently drops the lock and
        // can teleport the cursor onto a sibling window mid-click.
        // Inventory overlay needs a usable cursor + breaks the
        // lock so the mouse can leave the window without being
        // snapped back to the centre.
        let active =
            playing && self.has_focus && !self.any_panel_open();
        let grab = if active && self.settings.lock_cursor {
            if self.settings.hide_cursor {
                CursorGrabMode::Locked
            } else {
                CursorGrabMode::Confined
            }
        } else {
            CursorGrabMode::None
        };
        if raw.set_cursor_grab(grab).is_err() && grab == CursorGrabMode::Locked {
            let _ = raw.set_cursor_grab(CursorGrabMode::Confined);
        }
        raw.set_cursor_visible(!(active && self.settings.hide_cursor));
    }

    fn update_play(&mut self, ctx: &FrameContext<'_>, dt: f32) {
        // While the inventory is open we still tick physics (so the
        // player keeps falling / settling on the ground + the server
        // sees a live character) but freeze all player-driven inputs:
        // mouse-look, WASD, jump, hotbar wheel. This matches the
        // Minecraft convention - opening the inventory in multiplayer
        // never pauses the world for anyone.
        let input_active = !self.any_panel_open();

        if input_active {
            let dx = std::mem::take(&mut self.mouse_dx);
            let dy = std::mem::take(&mut self.mouse_dy);
            let sens = self.settings.mouse_sensitivity * 0.0035;
            self.player_yaw -= dx * sens;
            let dy_signed = if self.settings.invert_mouse_y {
                -dy
            } else {
                dy
            };
            self.player_pitch -= dy_signed * sens;
            let limit = std::f32::consts::FRAC_PI_2 - 0.05;
            self.player_pitch = self.player_pitch.clamp(-limit, limit);

            // Consume mouse wheel ticks - cycle hotbar slots (wheel up =
            // previous slot, matches Minecraft convention).
            let wheel = std::mem::take(&mut self.wheel_accum);
            let ticks = wheel.round() as i32;
            if ticks != 0 {
                let dir = if self.settings.invert_wheel { 1 } else { -1 };
                let mut slot = self.selected_slot as i32 + dir * ticks;
                slot = slot.rem_euclid(HOTBAR_SLOTS as i32);
                self.selected_slot = slot as usize;
            }
        } else {
            // Discard captured deltas so they don't snap the camera
            // when the menu closes.
            self.mouse_dx = 0.0;
            self.mouse_dy = 0.0;
            self.wheel_accum = 0.0;
        }

        let forward_horizontal =
            Vec3::new(self.player_yaw.cos(), 0.0, -self.player_yaw.sin()).normalize();
        // Right vector via the standard 90-deg horizontal rotation of
        // forward. For yaw=0 (forward = +X), right = +Z which matches
        // a player facing east strafing south.
        let right = Vec3::new(-forward_horizontal.z, 0.0, forward_horizontal.x);

        let mut move_axis: f32 = 0.0;
        let mut strafe: f32 = 0.0;
        // Movement speed multiplier: Shift = sneak (0.4x), Ctrl =
        // sprint (1.7x), no modifier = normal walk. Modifiers are
        // also gated on `input_active` so the menu doesn't sprint
        // the player while idle.
        let mut speed_mul = 1.0_f32;
        let mut sneaking = false;
        if input_active {
            if ctx.input.is_pressed(Key::W) {
                move_axis += 1.0;
            }
            if ctx.input.is_pressed(Key::S) {
                move_axis -= 1.0;
            }
            if ctx.input.is_pressed(Key::A) {
                strafe -= 1.0;
            }
            if ctx.input.is_pressed(Key::D) {
                strafe += 1.0;
            }
            if ctx.input.is_pressed(Key::ShiftLeft)
                || ctx.input.is_pressed(Key::ShiftRight)
            {
                speed_mul *= 0.4;
                sneaking = true;
            }
            if ctx.input.is_pressed(Key::ControlLeft)
                || ctx.input.is_pressed(Key::ControlRight)
            {
                // Stamina gate: sprint refuses to engage while
                // the player is below the reactivation
                // threshold so a fully-drained bar enforces a
                // brief walking penalty.
                let want_sprint = move_axis.abs() + strafe.abs() > 0.0;
                let stamina_ok = if self.sprinting {
                    self.stamina > 0.0
                } else {
                    self.stamina >= STAMINA_REACTIVATE
                };
                if want_sprint && stamina_ok {
                    speed_mul *= 1.7;
                    self.sprinting = true;
                } else {
                    self.sprinting = false;
                }
            } else {
                self.sprinting = false;
            }
        } else {
            self.sprinting = false;
        }
        // Stamina accounting: drain while sprinting, regen
        // otherwise. Clamped to [0, STAMINA_MAX] each tick.
        if self.sprinting {
            self.stamina = (self.stamina - STAMINA_DRAIN * dt).max(0.0);
        } else {
            self.stamina =
                (self.stamina + STAMINA_REGEN * dt).min(STAMINA_MAX);
        }
        // Smooth FOV lerp toward the per-state target. 60 deg
        // baseline, 70 deg while sprinting. The exponential lerp
        // factor `1 - exp(-rate * dt)` keeps the easing
        // framerate-independent.
        let target_fov = if self.sprinting {
            70_f32.to_radians()
        } else {
            60_f32.to_radians()
        };
        let lerp_rate = 8.0_f32;
        let alpha = 1.0 - (-lerp_rate * dt).exp();
        self.fov_y_rad += (target_fov - self.fov_y_rad) * alpha;
        let horizontal = (forward_horizontal * move_axis + right * strafe)
            .normalize_or_zero()
            * (PLAYER_SPEED * speed_mul);
        let mut hvel_x = horizontal.x;
        let mut hvel_z = horizontal.z;
        // Sneak edge protection: a grounded sneaking player can't
        // walk off a block. We probe per-axis along the candidate
        // delta and zero the component that would land the feet
        // over thin air. Both axes blocked = no horizontal move
        // at all (lets the player creep up to the edge safely).
        if sneaking && self.grounded {
            let test_step = 0.4;
            let test_x = self.player_pos.x + hvel_x.signum() * test_step;
            if hvel_x.abs() > 1e-3 && !self.has_floor_under(test_x, self.player_pos.z) {
                hvel_x = 0.0;
            }
            let test_z = self.player_pos.z + hvel_z.signum() * test_step;
            if hvel_z.abs() > 1e-3 && !self.has_floor_under(self.player_pos.x, test_z) {
                hvel_z = 0.0;
            }
        }
        self.player_vel.x = hvel_x;
        self.player_vel.z = hvel_z;

        if self.fly_mode {
            // Free-flight: Space = up, Shift = down. Velocity is
            // replaced (not integrated) so the player stops the
            // instant the key is released, the way Minecraft's
            // creative fly works. Ctrl held while flying doubles
            // the vertical thrust + horizontal speed multiplier
            // for fast traversal.
            let boost = input_active
                && (ctx.input.is_pressed(Key::ControlLeft)
                    || ctx.input.is_pressed(Key::ControlRight));
            let up = input_active && ctx.input.is_pressed(Key::Space);
            let down = input_active
                && (ctx.input.is_pressed(Key::ShiftLeft)
                    || ctx.input.is_pressed(Key::ShiftRight));
            let thrust = if boost { 16.0 } else { 8.0 };
            let v = if up && !down {
                thrust
            } else if down && !up {
                -thrust
            } else {
                0.0
            };
            self.player_vel.y = v;
            if boost {
                self.player_vel.x *= 1.7;
                self.player_vel.z *= 1.7;
            }
            self.grounded = false;
        } else {
            if input_active && ctx.input.is_pressed(Key::Space) && self.grounded {
                self.player_vel.y = JUMP_VELOCITY;
                self.grounded = false;
                // Jumping costs a flat 5 stamina so spamming the
                // bunny-hop drains the pool. Floor at 0.
                self.stamina = (self.stamina - 5.0).max(0.0);
                // Sprint-jump boost: add forward kick so a moving
                // jump covers more ground than a stand-still jump.
                if self.sprinting {
                    let fwd_h = Vec3::new(
                        self.player_yaw.cos(),
                        0.0,
                        -self.player_yaw.sin(),
                    )
                    .normalize_or_zero();
                    self.player_vel.x += fwd_h.x * 3.0;
                    self.player_vel.z += fwd_h.z * 3.0;
                }
            }
            self.player_vel.y += GRAVITY * dt;
        }

        // Footstep audio: fire a short click every ~0.4 s of
        // grounded horizontal motion. Sprint shortens the cadence
        // proportionally so it stays in sync with the leg.
        let speed = (self.player_vel.x * self.player_vel.x
            + self.player_vel.z * self.player_vel.z)
            .sqrt();
        // Advance the head-bob phase proportionally to horizontal
        // speed while grounded. Off the ground we let it ease
        // back to a quiet rest so a jump doesn't freeze the bob
        // mid-cycle.
        if self.grounded && speed > 0.5 {
            self.head_bob_phase += dt * (speed * 1.6 + 1.0);
        } else {
            // Bleed phase back to zero so the camera settles flat.
            self.head_bob_phase = (self.head_bob_phase
                - dt * std::f32::consts::PI)
                .max(0.0);
        }
        if input_active && self.grounded && speed > 0.5 {
            self.step_cooldown -= dt * (speed / PLAYER_SPEED);
            if self.step_cooldown <= 0.0 {
                const STEP_PERIOD: f32 = 0.42;
                self.step_cooldown = STEP_PERIOD;
                // Sample the voxel just below the player's feet so
                // the footstep sound matches the surface they're
                // actually standing on.
                let foot = [
                    self.player_pos.x.floor() as i32,
                    (self.player_pos.y - PLAYER_HEIGHT - 0.05).floor() as i32,
                    self.player_pos.z.floor() as i32,
                ];
                let under = self.world.get(foot);
                let mat = block_material(under);
                let key = format!("step:{mat}");
                let pos = [self.player_pos.x, self.player_pos.y, self.player_pos.z];
                self.sfx_at(&key, pos, |a, g| {
                    let _ = a.play_swish(0.10 * g);
                });
                // Sprint dust: a small puff per step matching the
                // surface colour. Walking sneak / normal speed
                // skips this so dust is a sprint-only flourish.
                if self.sprinting {
                    let color = self
                        .registry
                        .get(under)
                        .map(|d| d.color)
                        .unwrap_or([0.6, 0.55, 0.45]);
                    let foot_voxel = foot;
                    // Tiny burst that fades fast; reuses the
                    // existing particle system rather than adding
                    // a dedicated sprint emitter.
                    let base = [
                        self.player_pos.x,
                        self.player_pos.y - PLAYER_HEIGHT,
                        self.player_pos.z,
                    ];
                    let mut rng: u32 = 0xD15D_15D1
                        ^ ((foot_voxel[0] as u32).wrapping_mul(7))
                        ^ ((foot_voxel[2] as u32).wrapping_mul(13))
                        ^ (self.net_tick as u32);
                    let mut rand01 = || -> f32 {
                        rng ^= rng << 13;
                        rng ^= rng >> 17;
                        rng ^= rng << 5;
                        (rng as f32 / u32::MAX as f32).clamp(0.0, 1.0)
                    };
                    for _ in 0..4 {
                        let vx = (rand01() * 2.0 - 1.0) * 1.2;
                        let vy = rand01() * 0.6 + 0.2;
                        let vz = (rand01() * 2.0 - 1.0) * 1.2;
                        self.particles.particles.push(
                            infengine_particles::Particle {
                                position: base,
                                velocity: [vx, vy, vz],
                                color: [color[0], color[1], color[2], 0.85],
                                size: 0.08,
                                age: 0.0,
                                lifetime: 0.30,
                            },
                        );
                    }
                }
            }
        } else {
            // Reset cooldown so a step doesn't fire instantly the
            // moment we re-enter motion (keeps cadence natural).
            self.step_cooldown = 0.0;
        }

        // Distance-walked tracker. Uses XZ deltas only so
        // fall/jump motion doesn't inflate the counter.
        let cur_xz = (self.player_pos.x, self.player_pos.z);
        if let Some(prev) = self.last_walk_xz {
            let dx = cur_xz.0 - prev.0;
            let dz = cur_xz.1 - prev.1;
            let step = (dx * dx + dz * dz).sqrt();
            if step.is_finite() && step < 10.0 {
                self.distance_walked += step;
            }
        }
        self.last_walk_xz = Some(cur_xz);
        // Lightning state machine. Only fires while raining.
        // `next_lightning_at` counts down; on zero we trigger a
        // 0.18s flash + a thud and seed the next interval at a
        // randomized 10..=30 s window so strikes feel sporadic.
        if self.weather == 1 {
            self.next_lightning_at -= dt;
            if self.next_lightning_at <= 0.0 {
                self.lightning_flash = 0.18;
                let mut rng: u32 = ((self.net_tick as u32)
                    .wrapping_mul(0xC2B2_AE35))
                    ^ 0xDEAD_BEEF;
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                let bias = (rng as f32 / u32::MAX as f32).clamp(0.0, 1.0);
                self.next_lightning_at = 10.0 + bias * 20.0;
                // Distant thunder + chat note so peers know it
                // wasn't a graphics glitch.
                let pos = [
                    self.player_pos.x,
                    self.player_pos.y,
                    self.player_pos.z,
                ];
                self.sfx_at("thunder", pos, |a, g| {
                    let _ = a.play_thud(55.0, 0.7 * g);
                });
                self.chat_log.push((
                    std::time::Instant::now(),
                    "* lightning flashes overhead".to_string(),
                ));
            }
        }
        if self.lightning_flash > 0.0 {
            self.lightning_flash = (self.lightning_flash - dt).max(0.0);
        }
        // Precipitation spawner. weather=1 = rain (fast falling
        // blue streaks); weather=2 = snow (slow drifting white
        // flakes). Both stay purely visual. Skips spawning while
        // the player has solid ceiling overhead so being indoors
        // doesn't fill the room with rain.
        // Snow accumulation: while snowing, periodically blanket
        // a grass / dirt / sand voxel within ~12 blocks of the
        // player with a snow layer. Host-only so the edit goes
        // through the authoritative replication path. Limits to
        // one cell per fire so the world doesn't snap to a
        // uniform white sheet instantly.
        // Chicken passive laying (Q.3): every ~12 s the host
        // would drop one feather into its own inventory while a
        // live chicken existed. The loop ran unconditionally,
        // which is the "feathers spawn periodically in inventory
        // mid-air" bug the user reported. Now gated behind the
        // `/debug feathers on` toggle (defaults off); the
        // canonical way to get feathers is to kill a chicken (see
        // the mob death drop in `mob_ai`).
        let host_role = matches!(self.net, NetRole::Server { .. } | NetRole::Offline);
        let chicken_alive = self.mobs.iter().any(|m| m.kind == KIND_CHICKEN);
        if tick_chicken_feather_lay(
            self.debug_chicken_feathers,
            host_role,
            chicken_alive,
            &mut self.chicken_lay_accum,
            dt,
        ) {
            self.pickup(BlockId(19));
        }
        if self.weather == 2
            && matches!(self.net, NetRole::Server { .. } | NetRole::Offline)
        {
            self.snow_accum += dt;
            if self.snow_accum >= self.snow_rate.max(0.05) {
                self.snow_accum = 0.0;
                let r = 12i32;
                let mut rng = (self.net_tick as u32)
                    .wrapping_mul(0x9E37_79B1);
                let mut rand = || -> u32 {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    rng
                };
                let dx = (rand() % (2 * r as u32 + 1)) as i32 - r;
                let dz = (rand() % (2 * r as u32 + 1)) as i32 - r;
                let cx = self.player_pos.x.floor() as i32 + dx;
                let cz = self.player_pos.z.floor() as i32 + dz;
                let ceiling = (CHUNK_SIZE as i32) * VERTICAL_CHUNKS - 1;
                if let Some(sy) =
                    self.surface_y_below(cx, cz, ceiling)
                {
                    let surface = self.world.get([cx, sy, cz]).0;
                    // Walk upward looking for either an existing
                    // snow voxel to thicken or the first air cell
                    // to paint a fresh layer on. Cap stacking at
                    // 4 voxels per column so deep drifts don't
                    // pile up into the sky.
                    let mut target_y = sy + 1;
                    let mut stacked = 0u32;
                    while stacked < 16 {
                        let here = self
                            .world
                            .get([cx, target_y, cz])
                            .0;
                        if here == 0 {
                            break;
                        }
                        if here == 15 {
                            let snow_voxel = [cx, target_y, cz];
                            let bumped = if let Some(layers) = self
                                .snow_positions
                                .get_mut(&snow_voxel)
                                && *layers < 8
                            {
                                *layers += 1;
                                Some(*layers)
                            } else {
                                None
                            };
                            if let Some(new_layers) = bumped {
                                // Broadcast layer increment so
                                // MP peers grow the slab too.
                                let cfg =
                                    bincode::config::standard();
                                if let NetRole::Server {
                                    server, ..
                                } = &mut self.net
                                    && let Ok(buf) =
                                        bincode::serde::encode_to_vec(
                                            (snow_voxel, new_layers),
                                            cfg,
                                        )
                                {
                                    server.broadcast_bytes(
                                        "vx.snow", buf,
                                    );
                                }
                                target_y = i32::MIN;
                                break;
                            }
                            target_y += 1;
                            stacked += 1;
                            continue;
                        }
                        target_y = i32::MIN;
                        break;
                    }
                    let needs_solid_base = matches!(surface, 2 | 3 | 4 | 8 | 15);
                    if target_y != i32::MIN && needs_solid_base {
                        let voxel = [cx, target_y, cz];
                        let edit = EditMsg {
                            x: voxel[0],
                            y: voxel[1],
                            z: voxel[2],
                            block: 15,
                        };
                        self.world_set_tracked(voxel, BlockId(15));
                        let coords = Self::affected_chunks(voxel);
                        self.rebuild_chunks(&coords);
                        let cfg = bincode::config::standard();
                        if let NetRole::Server { server, .. } = &mut self.net
                        {
                            self.edit_history.push(edit);
                            if let Ok(buf) =
                                bincode::serde::encode_to_vec(edit, cfg)
                            {
                                server.broadcast_bytes("vx.edit", buf);
                            }
                        }
                    }
                }
            }
        }
        let mut sheltered = false;
        if self.weather == 1 || self.weather == 2 {
            let px = self.player_pos.x.floor() as i32;
            let pz = self.player_pos.z.floor() as i32;
            let py = self.player_pos.y.floor() as i32;
            for dy in 1..15 {
                if self.world.get([px, py + dy, pz]) != BlockId::AIR {
                    sheltered = true;
                    break;
                }
            }
        }
        if (self.weather == 1 || self.weather == 2) && !sheltered {
            const FALL_PER_FRAME: u32 = 20;
            let mut rng: u32 = ((self.net_tick as u32)
                .wrapping_mul(0x9E37_79B1))
                ^ 0xCAFE_F00D;
            let mut rand01 = || -> f32 {
                rng ^= rng << 13;
                rng ^= rng >> 17;
                rng ^= rng << 5;
                (rng as f32 / u32::MAX as f32).clamp(0.0, 1.0)
            };
            let base = [
                self.player_pos.x,
                self.player_pos.y + 14.0,
                self.player_pos.z,
            ];
            for _ in 0..FALL_PER_FRAME {
                let dx = (rand01() * 2.0 - 1.0) * 22.0;
                let dz = (rand01() * 2.0 - 1.0) * 22.0;
                let (vel, color, size, life) = if self.weather == 1 {
                    (
                        [0.0_f32, -16.0, 0.0_f32],
                        [0.55, 0.65, 0.85, 0.65],
                        0.05,
                        1.2,
                    )
                } else {
                    // Snow: small lateral drift + slow descent.
                    let drift_x = (rand01() * 2.0 - 1.0) * 0.6;
                    let drift_z = (rand01() * 2.0 - 1.0) * 0.6;
                    (
                        [drift_x, -1.6, drift_z],
                        [0.92, 0.95, 1.00, 0.85],
                        0.08,
                        7.0,
                    )
                };
                self.particles.particles.push(
                    infengine_particles::Particle {
                        position: [base[0] + dx, base[1], base[2] + dz],
                        velocity: vel,
                        color,
                        size,
                        age: 0.0,
                        lifetime: life,
                    },
                );
            }
        }
        // Low-HP heartbeat thump. Interval shrinks from ~1.2 s
        // at HP=5 down to ~0.5 s at HP=1. Above HP=5 we leave the
        // accumulator at 0 so re-entering the danger zone fires
        // the first beat promptly.
        if self.player_health > 0.0 && self.player_health < 5.0 {
            self.heartbeat_accum += dt;
            let interval = (0.5 + 0.2 * self.player_health).clamp(0.5, 1.4);
            if self.heartbeat_accum >= interval {
                self.heartbeat_accum = 0.0;
                let pos = [self.player_pos.x, self.player_pos.y, self.player_pos.z];
                let vol_scale = (1.0 - (self.player_health / 5.0)).clamp(0.2, 1.0);
                self.sfx_at("heartbeat", pos, move |a, _g| {
                    let _ = a.play_thud(70.0, 0.28 * vol_scale);
                });
            }
        } else {
            self.heartbeat_accum = 0.0;
        }

        // Swim mechanics. While the player's AABB overlaps a
        // water voxel: cap fall speed, scale horizontal velocity
        // (drag), and let Space push them upward (buoyancy +
        // swim-up). Skipped in fly mode so /fly through water
        // still glides freely.
        let liquid_under = if self.fly_mode {
            None
        } else {
            submerged_liquid(&self.world, self.player_pos)
        };
        if let Some(liquid) = liquid_under {
            // Every swim parameter comes from the `Liquid` row -
            // water reads light + smooth, lava drags + barely
            // floats. Adding a new liquid means filling these
            // fields; no fluid-specific code in the swim loop.
            if self.player_vel.y < liquid.swim_fall_cap {
                self.player_vel.y = liquid.swim_fall_cap;
            }
            self.player_vel.x *= liquid.swim_horiz_drag;
            self.player_vel.z *= liquid.swim_horiz_drag;
            // Current: flowing cells push the player downstream
            // at the liquid's `current_strength`. Source cells
            // (level 0 with no lower-level neighbour) return a
            // zero vector so a pond doesn't drag the player.
            let foot = [
                self.player_pos.x.floor() as i32,
                (self.player_pos.y - PLAYER_HEIGHT * 0.5).floor() as i32,
                self.player_pos.z.floor() as i32,
            ];
            let foot_block = self.world.get(foot);
            let mut foot_in_flow = false;
            if foot_block.0 == liquid.block_id
                && liquid.current_strength > 0.0
            {
                let dir = self.liquid_flow_dir(foot, liquid.block_id);
                if dir[0] != 0.0 || dir[1] != 0.0 {
                    let push = liquid.current_strength * dt * 6.0;
                    self.player_vel.x += dir[0] * push;
                    self.player_vel.z += dir[1] * push;
                    foot_in_flow = true;
                }
            }
            // Flowing-water ambience: when standing in a flow
            // voxel, periodically play a soft running-water
            // sting near the foot position. Source ponds stay
            // silent (no flow direction).
            self.flow_sfx_accum -= dt;
            if foot_in_flow && self.flow_sfx_accum <= 0.0 {
                self.flow_sfx_accum = 0.55;
                let center = [
                    foot[0] as f32 + 0.5,
                    foot[1] as f32 + 0.5,
                    foot[2] as f32 + 0.5,
                ];
                self.sfx_at("water_flow", center, |a, g| {
                    let _ = a.play_swish(0.12 * g);
                });
            }
            if input_active && ctx.input.is_pressed(Key::Space) {
                self.player_vel.y =
                    self.player_vel.y.max(liquid.swim_jump_vel);
            } else {
                self.player_vel.y += liquid.swim_buoyancy * dt;
                if self.player_vel.y > 1.0 {
                    self.player_vel.y = 1.0;
                }
            }
            // Reset fall-damage tracker so falling into the
            // liquid doesn't punish on the next grounded
            // transition.
            self.fall_start_y = None;
            // Submerged damage tick (lava burns, future acid).
            if liquid.submerged_damage_per_sec > 0.0 {
                self.apply_player_damage_kind(
                    liquid.submerged_damage_per_sec * dt,
                    DamageKind::Burn,
                );
            }
        }
        // Breath: only the eye-submerged check drains air -
        // mirrors MC's `getEyeInFluid() == WATER`. Wading
        // ankle- or waist-deep doesn't suffocate the player;
        // only fully-under does. Refill 4x as fast on the
        // surface so a quick breath buys plenty of dive time.
        // Independent of `liquid_under` so the surfacing
        // transition fires even when feet are still wet.
        let eye_liquid = if self.fly_mode {
            None
        } else {
            submerged_liquid_eye(&self.world, self.player_pos)
        };
        if let Some(eye_liq) = eye_liquid
            && eye_liq.drains_air
        {
            self.player_air = (self.player_air - dt).max(0.0);
            if self.player_air <= 0.0 {
                self.apply_player_damage_kind(
                    DROWN_DAMAGE_PER_SEC * dt,
                    DamageKind::Drown,
                );
            }
        } else {
            self.player_air =
                (self.player_air + dt * 4.0).min(AIR_MAX_SEC);
        }
        let in_water = liquid_under.is_some();
        // Splash on the air -> water transition: short droplet
        // burst + click SFX. Quieter cue going the other way.
        if in_water != self.was_in_water {
            let base = [
                self.player_pos.x,
                self.player_pos.y - PLAYER_HEIGHT * 0.5,
                self.player_pos.z,
            ];
            if in_water {
                // Splash synth: a low thud (mass hitting water)
                // layered with a high noise swish (the spray).
                // Much more water-like than a single click.
                self.sfx_at("splash", base, |a, g| {
                    let _ = a.play_thud(140.0, 0.20 * g);
                    let _ = a.play_swish(0.18 * g);
                });
                let mut rng: u32 = ((self.net_tick as u32)
                    .wrapping_mul(0xBEEF_0BAD))
                    ^ 0x5EA_5EA;
                let mut rand01 = || -> f32 {
                    rng ^= rng << 13;
                    rng ^= rng >> 17;
                    rng ^= rng << 5;
                    (rng as f32 / u32::MAX as f32).clamp(0.0, 1.0)
                };
                for _ in 0..18 {
                    let vx = (rand01() * 2.0 - 1.0) * 2.4;
                    let vy = rand01() * 2.6 + 1.5;
                    let vz = (rand01() * 2.0 - 1.0) * 2.4;
                    self.particles.particles.push(
                        infengine_particles::Particle {
                            position: base,
                            velocity: [vx, vy, vz],
                            color: [0.65, 0.80, 0.95, 0.95],
                            size: 0.07,
                            age: 0.0,
                            lifetime: 0.45,
                        },
                    );
                }
            } else {
                self.sfx_at("splash_out", base, |a, g| {
                    let _ = a.play_swish(0.14 * g);
                });
            }
        }
        self.was_in_water = in_water;
        // Axis-resolved voxel AABB collision.
        let was_grounded = self.grounded;
        let delta = self.player_vel * dt;
        let (new_pos, hit_floor, hit_ceiling) = sweep_aabb(
            &self.world,
            &self.snow_positions,
            self.player_pos,
            delta,
        );
        self.player_pos = new_pos;
        if hit_floor {
            self.player_vel.y = 0.0;
            self.grounded = true;
        } else {
            self.grounded = false;
        }
        if hit_ceiling {
            self.player_vel.y = 0.0;
        }
        // Fall-damage tracking. Note the world-Y the moment we
        // first leave the ground, then on the landing transition
        // compute the drop height + apply damage.
        if was_grounded && !self.grounded {
            self.fall_start_y = Some(self.player_pos.y);
        }
        if !was_grounded
            && self.grounded
            && let Some(start_y) = self.fall_start_y.take()
        {
            let drop = (start_y - self.player_pos.y).max(0.0);
            // Fly mode disables fall damage so toggling /fly off
            // mid-air doesn't immediately murder the player.
            if drop > SAFE_FALL && !self.fly_mode {
                let landing = [
                    self.player_pos.x.floor() as i32,
                    (self.player_pos.y - PLAYER_HEIGHT - 0.05).floor() as i32,
                    self.player_pos.z.floor() as i32,
                ];
                // Landing on snow halves the impact - snow
                // cushion mechanic.
                let on_snow = self.world.get(landing).0 == 15;
                let mut dmg = drop - SAFE_FALL;
                if on_snow {
                    dmg *= 0.5;
                }
                self.apply_player_damage(dmg);
                self.spawn_break_particles(landing);
            }
        }
        // Cactus contact damage. Check the 8 voxels around the
        // player body for cactus id 27; if any overlap, apply 1
        // HP every 0.6 s.
        let cactus_touch = {
            let pos = self.player_pos;
            let foot_y = (pos.y - PLAYER_HEIGHT - 0.05).floor() as i32;
            let mid_y = (pos.y - PLAYER_HEIGHT * 0.5).floor() as i32;
            let px = pos.x.floor() as i32;
            let pz = pos.z.floor() as i32;
            (-1..=1).any(|dx| {
                (-1..=1).any(|dz| {
                    if dx == 0 && dz == 0 {
                        return false;
                    }
                    let v_lo = [px + dx, foot_y, pz + dz];
                    let v_hi = [px + dx, mid_y, pz + dz];
                    self.world.get(v_lo).0 == 27 || self.world.get(v_hi).0 == 27
                })
            })
        };
        if cactus_touch {
            self.cactus_damage_accum += dt;
            if self.cactus_damage_accum >= 0.6 {
                self.cactus_damage_accum = 0.0;
                self.apply_player_damage(1.0);
            }
        } else {
            self.cactus_damage_accum = 0.0;
        }
        // Hurt-flash decay.
        if self.hurt_flash > 0.0 {
            self.hurt_flash = (self.hurt_flash - dt).max(0.0);
        }
        if self.invuln_remaining > 0.0 {
            self.invuln_remaining = (self.invuln_remaining - dt).max(0.0);
        }
        // HP regen: tick the grace timer; once the player has gone
        // `REGEN_DELAY` seconds without damage, drip HP back in at
        // `REGEN_RATE` HP/s until full.
        if self.player_health > 0.0 && self.player_health < PLAYER_MAX_HEALTH {
            self.regen_accum += dt;
            if self.regen_accum > REGEN_DELAY {
                let gain = REGEN_RATE * dt;
                self.player_health =
                    (self.player_health + gain).min(PLAYER_MAX_HEALTH);
            }
        }

        if self.player_pos.y < -20.0 {
            // Fell through the world - respawn above origin. Acts
            // as a soft "death": storage rows wipe so falling has
            // real cost, hotbar is kept so the player isn't reduced
            // to bare hands. Locally we always wipe (optimistic);
            // in MP client mode we also fire a `vx.fellout` so the
            // server's authoritative inventory mirrors the penalty.
            tracing::warn!(
                pos = ?[self.player_pos.x, self.player_pos.y, self.player_pos.z],
                "player fell through the world - respawning"
            );
            self.player_pos = self.safe_spawn();
            self.player_vel = Vec3::ZERO;
            let mut dropped = 0usize;
            for slot in self.inventory[HOTBAR_SLOTS..].iter_mut() {
                if slot.take().is_some() {
                    dropped += 1;
                }
            }
            for slot in self.craft_grid.iter_mut() {
                if slot.take().is_some() {
                    dropped += 1;
                }
            }
            if dropped > 0 {
                tracing::info!(dropped, "fall-out penalty: cleared storage stacks");
                self.spawn_break_particles([0, 32, 0]);
            }
            if let NetRole::Client { client, .. } = &self.net {
                let _ = client.send_bytes("vx.fellout", Vec::new());
            }
        }
    }

    fn render_ui(&mut self, window: &EngineWindow) {
        let state = self.state;
        let fps = self.fps_ema;
        let show_fps = self.settings.show_fps;
        let dim = self.current_dim.clone();
        let day_time = self.day_time;
        let player_health = self.player_health;
        let stamina_frac =
            (self.stamina / STAMINA_MAX).clamp(0.0, 1.0);
        let air_frac =
            (self.player_air / AIR_MAX_SEC).clamp(0.0, 1.0);
        // Underwater / lava view tint. Triggered only when the
        // EYE voxel is in a liquid (head fully submerged); wading
        // through ankle-deep water doesn't paint the screen. The
        // body-AABB swim mechanics still kick in earlier - drag
        // / buoyancy / air drain react to any overlap, only the
        // overlay needs full submersion.
        let submerged_tint: Option<[f32; 4]> = if self.fly_mode {
            None
        } else {
            submerged_liquid_eye(&self.world, self.player_pos)
                .map(|l| l.submerged_tint)
        };
        let player_hurt_flash = self.hurt_flash;
        let invuln_remaining = self.invuln_remaining;
        let show_debug = self.show_debug;
        let hud_visible = self.hud_visible;
        let kill_count = self.kill_count;
        let chat_open = self.chat_open;
        let held_tool_label: Option<&'static str> = self
            .inventory
            .get(self.selected_slot)
            .copied()
            .unwrap_or(None)
            .and_then(|(b, _)| tool_label(b));
        let weather_label: Option<&'static str> = match self.weather {
            1 => Some("Rain"),
            2 => Some("Snow"),
            _ => None,
        };
        let biome_at_player = biome_at(self.player_pos.x, self.player_pos.z, world_seed());
        let spawn_xz: (f32, f32) = match self.spawn_pos {
            Some(p) => (p[0], p[2]),
            None => (0.0, 0.0),
        };
        // `biome_at` only returns Plains/Mountain/Desert under the
        // Classic + Amplified generators (which are the only path
        // that reaches this HUD-overlay block today). The extended
        // biomes the Realistic generator produces still fall back
        // to a label via `Biome::label()` so future climate-driven
        // HUD work doesn't have to thread a second enum.
        let biome_name = biome_at_player.label();
        // Snapshot recent chat lines (newest last) along with their
        // age so the renderer can fade older entries.
        let chat_now = std::time::Instant::now();
        let chat_lines: Vec<(String, f32, bool)> = {
            let own_lower = self.player_name.to_lowercase();
            self.chat_log
                .iter()
                .map(|(t, line)| {
                    let mention =
                        !own_lower.is_empty() && line.to_lowercase().contains(&own_lower);
                    (
                        line.clone(),
                        chat_now.saturating_duration_since(*t).as_secs_f32(),
                        mention,
                    )
                })
                .collect()
        };
        let mut chat_send_now = false;
        // View-projection for projecting mob HP bars to screen.
        // Built from the same camera state the world pass uses;
        // captured here so the egui closure doesn't need renderer
        // access.
        let hud_cam_pos = self.player_pos;
        let hud_forward = Vec3::new(
            self.player_yaw.cos() * self.player_pitch.cos(),
            self.player_pitch.sin(),
            -self.player_yaw.sin() * self.player_pitch.cos(),
        );
        let hud_aspect =
            self.viewport.0.max(1) as f32 / self.viewport.1.max(1) as f32;
        let hud_view_proj = Mat4::perspective_rh(
            self.fov_y_rad,
            hud_aspect,
            0.1,
            500.0,
        ) * Mat4::look_at_rh(hud_cam_pos, hud_cam_pos + hud_forward, Vec3::Y);
        // Sky visibility for the star field: the stars are 2D egui
        // circles on the Background layer (no depth test) so they
        // would otherwise render through stone / ceilings inside
        // caves or enclosed builds. Compute once here against
        // `self.world` so the egui closure doesn't need a world
        // capture. Looks for a non-passable solid voxel within
        // 64 cells directly above the player.
        let sky_blocked = (1..=64).any(|dy| {
            let above = [
                self.player_pos.x.floor() as i32,
                (self.player_pos.y as i32) + dy,
                self.player_pos.z.floor() as i32,
            ];
            let b = self.world.get(above);
            !b.is_air()
                && !block_is_ray_passable(b)
                && !block_is_aabb_passable(b)
        });
        // Star field: fades in as the sun drops below the horizon.
        // The raw sun elevation (un-clamped) gives a smooth
        // night-factor in [0, 1] when the sun is below the horizon.
        let phase = (self.day_time / DAY_LENGTH_S).rem_euclid(1.0);
        let raw_elev = (phase * std::f32::consts::TAU).sin();
        let night_factor = (-raw_elev).clamp(0.0, 1.0);
        // Mob HP snapshot: (pos, current, max). Only mobs that
        // are damaged + not dying show a bar so the world doesn't
        // get cluttered with full-health bars.
        // Targeted-mob info shown next to the crosshair: kind name
        // + current/max HP. Drives a small label so the player can
        // see how many more hits a sheep / zombie needs.
        let aim_forward = Vec3::new(
            self.player_yaw.cos() * self.player_pitch.cos(),
            self.player_pitch.sin(),
            -self.player_yaw.sin() * self.player_pitch.cos(),
        );
        let aim_origin = self.player_pos;
        // Targeted-block label: name + (if actively mining) the
        // progress percentage. Hidden when the crosshair isn't on
        // anything breakable.
        let held_for_label = self
            .inventory
            .get(self.selected_slot)
            .copied()
            .unwrap_or(None)
            .map(|(b, _)| b)
            .unwrap_or(BlockId::AIR);
        let targeted_block_info: Option<(String, Option<u8>)> =
            self.target.and_then(|hit| {
                let b = self.world.get(hit.voxel);
                if b == BlockId::AIR {
                    return None;
                }
                let name = block_type(b.0).map(|t| t.name).unwrap_or("?");
                let pct = self.mining.as_ref().and_then(|m| {
                    if m.voxel == hit.voxel && m.block == b {
                        Some(((m.progress / m.total).clamp(0.0, 1.0) * 100.0)
                            as u8)
                    } else {
                        None
                    }
                });
                // Append a hint about the recommended tool when
                // the held one doesn't match the block category.
                let cat = block_category(b);
                let hint = match cat {
                    BlockCategory::Stone
                        if tool_tier(held_for_label) == 0 =>
                    {
                        " (need pickaxe)"
                    }
                    BlockCategory::Soil
                        if shovel_tier(held_for_label) == 0 =>
                    {
                        " (use shovel)"
                    }
                    BlockCategory::Wood
                        if axe_tier(held_for_label) == 0 =>
                    {
                        " (use axe)"
                    }
                    _ => "",
                };
                Some((format!("{name}{hint}"), pct))
            });
        let targeted_mob_info: Option<(&'static str, u8, u8)> = self
            .raycast_mob(aim_origin, aim_forward, MOB_ATTACK_REACH * 1.5)
            .and_then(|(id, _)| self.remote_mobs.get(&id).copied())
            .map(|m| {
                let max = match m.kind {
                    KIND_ZOMBIE => ZOMBIE_MAX_HEALTH as u8,
                    KIND_CHICKEN => 4u8,
                    KIND_PIG => 10u8,
                    _ => MOB_MAX_HEALTH as u8,
                };
                let name = match m.kind {
                    KIND_ZOMBIE => "Zombie",
                    KIND_CHICKEN => "Chicken",
                    KIND_PIG => "Pig",
                    _ => "Sheep",
                };
                (name, m.health, max)
            });
        let mob_bars: Vec<([f32; 3], u8, u8)> = self
            .remote_mobs
            .values()
            .filter(|m| {
                let dying = self.mob_dying.get(&m.id).copied().unwrap_or(0.0);
                let max = match m.kind {
                    KIND_ZOMBIE => ZOMBIE_MAX_HEALTH as u8,
                    KIND_CHICKEN => 4u8,
                    KIND_PIG => 10u8,
                    _ => MOB_MAX_HEALTH as u8,
                };
                m.health > 0 && m.health < max && dying <= 0.0
            })
            .map(|m| {
                let max = match m.kind {
                    KIND_ZOMBIE => ZOMBIE_MAX_HEALTH as u8,
                    KIND_CHICKEN => 4u8,
                    KIND_PIG => 10u8,
                    _ => MOB_MAX_HEALTH as u8,
                };
                (m.pos, m.health, max)
            })
            .collect();
        // Remote nameplate snapshot: world-space pose + display
        // name for each other player so the egui pass can project
        // them onto the screen. Skips own_id so the local player
        // never sees a tag on their own head.
        let own_id_for_plates: u32 = match &self.net {
            NetRole::Offline => u32::MAX,
            NetRole::Server { .. } => 0,
            NetRole::Client { client, .. } => client.client_id.unwrap_or(0),
        };
        let plate_now = std::time::Instant::now();
        let mut nameplates: Vec<([f32; 3], String, [u8; 3])> = self
            .remote_interp
            .iter()
            .filter(|(id, _)| **id != own_id_for_plates)
            .map(|(id, r)| {
                let p = r.sample(plate_now);
                let name = self
                    .player_names
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| format!("p{id}"));
                // Stable name colour per id: hash-derived hue,
                // brightened so the text reads against the sky.
                let h = (id.wrapping_mul(2654435761)) as f32 / u32::MAX as f32;
                let tau = std::f32::consts::TAU;
                let r_c = (0.6 + 0.4 * (h * tau).sin().abs()).min(1.0);
                let g_c =
                    (0.6 + 0.4 * ((h + 0.33) * tau).sin().abs()).min(1.0);
                let b_c =
                    (0.6 + 0.4 * ((h + 0.66) * tau).sin().abs()).min(1.0);
                let color = [
                    (r_c * 255.0) as u8,
                    (g_c * 255.0) as u8,
                    (b_c * 255.0) as u8,
                ];
                (p.pos, name, color)
            })
            .collect();
        // In third-person we also draw the local player's
        // nameplate so the camera-pull-back body has its label.
        if self.camera_mode != 0 {
            nameplates.push((
                [
                    self.player_pos.x,
                    self.player_pos.y,
                    self.player_pos.z,
                ],
                self.player_name.clone(),
                [180, 230, 180],
            ));
        }
        // Pickup toast snapshot: trim entries older than the
        // fade window so the HUD doesn't keep stale text alive.
        // We do the housekeeping here so the egui closure sees a
        // clean list.
        let toast_now = std::time::Instant::now();
        let toast_lifetime = 2.4_f32;
        self.pickup_toasts
            .retain(|(t, _, _)| toast_now.duration_since(*t).as_secs_f32() <= toast_lifetime);
        let pickup_snapshot: Vec<(f32, String, [f32; 3])> = self
            .pickup_toasts
            .iter()
            .rev()
            .take(6)
            .map(|(t, b, c)| {
                let age = toast_now.duration_since(*t).as_secs_f32();
                let alpha = (1.0 - (age / toast_lifetime)).clamp(0.0, 1.0);
                let name = block_type(b.0).map(|t| t.name).unwrap_or("?");
                let color = self
                    .registry
                    .get(*b)
                    .map(|d| d.color)
                    .unwrap_or([1.0, 1.0, 1.0]);
                (alpha, format!("+{c} {name}"), color)
            })
            .collect();
        // Tab playerlist snapshot: pairs of (display_name,
        // is_self). Drawn as a modal centered overlay while Tab
        // is held.
        let playerlist_visible = self.tab_held && !matches!(self.net, NetRole::Offline);
        let playerlist: Vec<(String, bool)> = if playerlist_visible {
            let mut list: Vec<(String, bool)> = self
                .player_names
                .iter()
                .map(|(id, n)| (n.clone(), *id == own_id_for_plates))
                .collect();
            // Self first, then alphabetical so the order is stable
            // across ticks.
            list.sort_by(|a, b| match (a.1, b.1) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
            });
            list
        } else {
            Vec::new()
        };
        let dbg_player = self.player_pos;
        let dbg_yaw = self.player_yaw;
        let dbg_pitch = self.player_pitch;
        let dbg_chunks = self.chunk_meshes.len();
        let dbg_grounded = self.grounded;
        let dbg_mobs = self.remote_mobs.len();
        let dbg_particles = self.particles.live_count();
        let dbg_fly = self.fly_mode;
        let dbg_cam = match self.camera_mode {
            0 => "1st",
            1 => "3rd back",
            _ => "3rd front",
        };
        let dbg_playtime = self
            .session_started
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);
        let dbg_pending = self.pending_chunks.len();
        let dbg_net = match &self.net {
            NetRole::Offline => "offline".to_string(),
            NetRole::Server { server, poses, .. } => {
                format!(
                    "server (cli={}, poses={})",
                    server.client_count(),
                    poses.len(),
                )
            }
            NetRole::Client {
                client,
                pending_edits,
                pending_inv_actions,
                ..
            } => format!(
                "client (id={:?}, edits={}, inv_acts={})",
                client.client_id,
                pending_edits.len(),
                pending_inv_actions.len(),
            ),
        };
        let settings_edit = &mut self.settings;
        // Live-edit flag the F6 panel writes back when the user
        // clicks its window close button. Mirrors back to
        // `self.show_debug_panel` after the egui closure exits.
        let mut dbg_panel_open = self.show_debug_panel;
        let dbg_panel_was_open = dbg_panel_open;
        // Mutable refs the debug panel writes to. Snapshot the
        // weather pre-edit so we can detect changes after the
        // closure and rebroadcast.
        let dbg_day_time = &mut self.day_time;
        let dbg_weather = &mut self.weather;
        let dbg_fov_rad = &mut self.fov_y_rad;
        let dbg_freeze_time = &mut self.freeze_time;
        let dbg_fly_mode = &mut self.fly_mode;
        let dbg_god_mode = &mut self.god_mode;
        let dbg_mob_cap = &mut self.mob_cap;
        let dbg_snow_rate = &mut self.snow_rate;
        let dbg_liquid_bias = &mut self.liquid_bias_drops;
        let weather_before = *dbg_weather;
        // Lazy-load the egui atlas texture on first render after
        // on_start has populated the pixel cache. We use NEAREST
        // sampling so the 16x16 tiles stay crisp at slot size.
        if self.egui_atlas.is_none()
            && let (Some(ui), Some((aw, ah, rgba))) = (self.ui.as_ref(), self.atlas_rgba.as_ref())
        {
            let img = egui::ColorImage::from_rgba_unmultiplied(
                [*aw as usize, *ah as usize],
                rgba,
            );
            let handle = ui.context().load_texture(
                "voxel_atlas_egui",
                img,
                egui::TextureOptions::NEAREST,
            );
            self.egui_atlas = Some(handle);
        }
        let atlas_tex_id: Option<egui::TextureId> =
            self.egui_atlas.as_ref().map(|h| h.id());
        // Snapshot tuple: (color, count, uv_rect_for_top_face). UV
        // is only used when the egui atlas texture is loaded;
        // otherwise we fall back to a solid color swatch.
        let icon = |slot: Option<(BlockId, u8)>| -> Option<SlotIcon> {
            slot.and_then(|(b, c)| {
                let bt = block_type(b.0);
                self.registry
                    .get(b)
                    .map(|d| (d.color, c, atlas_uv_rect(b, [0, 1, 0]), bt.map(|t| t.name).unwrap_or("?")))
            })
        };
        let hotbar_snapshot: Vec<Option<SlotIcon>> = self.inventory
            [..HOTBAR_SLOTS]
            .iter()
            .map(|s| icon(*s))
            .collect();
        let inventory_open = self.inventory_open;
        let mut inv_open_local = self.inventory_open;
        let mp_menu_open = self.mp_menu_open;
        let mut mp_menu_open_local = self.mp_menu_open;
        // Chest UI snapshot. Cloned here so the egui closure can
        // render a grid against a stable view; applied actions
        // mutate `self.chests` after the closure finishes.
        let chest_open_pos = self.chest_open;
        let chest_snapshot: Option<container::Container> = self
            .chest_open
            .as_ref()
            .and_then(|p| self.chests.get(p).cloned());
        // Sentinel that the egui close-button + ESC inside the
        // chest panel can flip; we read it back after the closure
        // to clear `chest_open`.
        let mut chest_open_local = chest_open_pos.is_some();
        let chest_held_snapshot = self.chest_held;
        // Furnace UI snapshot. Same shape as the chest snapshot
        // above, plus a clone of the live runtime triple so the
        // panel can draw the fuel + cook bars.
        let furnace_open_pos = self.furnace_open;
        let furnace_snapshot: Option<container::Container> = self
            .furnace_open
            .as_ref()
            .and_then(|p| self.furnaces.get(p).cloned());
        let furnace_runtime_snapshot: block::furnace::FurnaceRuntime = self
            .furnace_open
            .as_ref()
            .and_then(|p| self.furnace_runtimes.get(p).copied())
            .unwrap_or_default();
        let mut furnace_open_local = furnace_open_pos.is_some();
        let furnace_held_snapshot = self.furnace_held;
        let mut furnace_grid_actions: Vec<(bool, container::ui::GridAction)> = Vec::new();
        let mut furnace_held_local = furnace_held_snapshot;
        // Plain inventory snapshot as Option<(u16,u8)> for the
        // chest-UI's player-side grid - independent of the
        // existing DnD widget snapshot which carries icon meta.
        let inv_plain_snapshot: Vec<Option<(u16, u8)>> = self
            .inventory
            .iter()
            .map(|s| s.map(|(b, c)| (b.0, c)))
            .collect();
        let mp_role_label: Option<&'static str> = match &self.net {
            NetRole::Offline => None,
            NetRole::Server { .. } => Some("hosting server"),
            NetRole::Client { .. } => Some("connected to server"),
        };
        let inventory_snapshot: Vec<Option<SlotIcon>> = if inventory_open {
            self.inventory[HOTBAR_SLOTS..]
                .iter()
                .map(|s| icon(*s))
                .collect()
        } else {
            Vec::new()
        };
        let craft_snapshot: [Option<SlotIcon>; 4] =
            std::array::from_fn(|i| icon(self.craft_grid[i]));
        let craft_match: Option<(BlockId, u8, &'static str)> =
            match_recipe_named(&self.craft_grid);
        let craft_output_snapshot = craft_match.and_then(|(b, c, _)| {
            let bt = block_type(b.0);
            self.registry.get(b).map(|d| {
                (
                    d.color,
                    c,
                    atlas_uv_rect(b, [0, 1, 0]),
                    bt.map(|t| t.name).unwrap_or("?"),
                )
            })
        });
        let craft_recipe_name: Option<&'static str> = craft_match.map(|(_, _, n)| n);
        let selected_slot = self.selected_slot;
        // Click actions to apply after the egui closure exits (the
        // closure can't mutate `self` directly because the renderer
        // already borrowed it).
        let mut inv_actions: Vec<InvAction> = Vec::new();
        // Chest-UI side channels. The egui closure pushes one
        // `(side, GridAction)` per click; we drain them post-egui
        // and route to `block::chest::apply_chest_action` /
        // `apply_player_action`. `side` = false means a click on
        // the chest grid, `true` = a click on the player-inv grid.
        let mut chest_grid_actions: Vec<(bool, container::ui::GridAction)> =
            Vec::new();
        let mut chest_held_local = chest_held_snapshot;
        let mut action_start = false;
        let mut action_resume = false;
        // Set by the recipe-book output slot click handler; drained
        // after the egui closure exits so the craft-grid mutation
        // can borrow `self` exclusively.
        let mut prefill_recipe_idx: Option<usize> = None;
        let mut action_quit = false;
        let mut action_menu_to_settings = false;
        let mut action_pause_to_settings = false;
        let mut action_back_settings = false;
        let mut action_main_menu = false;
        let mut action_to_direct = false;
        let mut action_to_browser = false;
        let mut action_to_host = false;
        let mut action_join_direct = false;
        let mut action_join_browser: Option<String> = None;
        let mut action_browser_refresh = false;
        let mut action_host_start = false;
        // MP overlay actions: continue playing (close overlay) /
        // explicit disconnect-and-return-to-main-menu.
        let mut action_mp_resume = false;
        let mut action_mp_disconnect = false;
        let mut action_mp_quit = false;
        // Worlds-picker actions (Phase B.7a scaffolding). Drained
        // after the egui closure exits so the slug rewrite + slot
        // deletion happen under an exclusive borrow.
        let mut action_new_world = false;
        let mut action_load_world: Option<String> = None;
        let mut action_delete_world: Option<String> = None;
        let new_world_name = &mut self.new_world_name_buf;
        // Snapshot the on-disk worlds list once per UI frame; the
        // picker doesn't need to re-scan on every redraw.
        let worlds_list: Vec<world_save::layout::WorldSummary> =
            world_save::layout::list_worlds();
        let current_slug = self.current_world_slug.clone();
        let direct_buf = &mut self.direct_addr_buf;
        let host_name = &mut self.host_name_buf;
        let host_port = &mut self.host_port_buf;
        let host_announce = &mut self.host_announce;
        let master_addr = &mut self.master_addr;
        let browser_servers = &self.browser_servers;
        let browser_error = &self.browser_error;
        let browser_last_refresh = self.browser_last_refresh;
        let browser_querying = self.browser_query_rx.is_some();
        // Mutable borrow for the chat input box captured by the
        // egui closure. Taken last so earlier self.* reads
        // (raycast_mob, registry, etc.) finish before this
        // exclusive borrow begins.
        let chat_buf = &mut self.chat_input;
        let chat_history_ref = &self.chat_history;
        let chat_history_idx = &mut self.chat_history_idx;
        let player_name_buf = &mut self.player_name;

        let Some(ui) = self.ui.as_mut() else {
            return;
        };
        ui.run(window.raw(), |ctx| match state {
            AppState::MainMenu => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(60.0);
                        ui.heading("InfEngine - Voxel Demo");
                        ui.add_space(20.0);
                        // Worldgen picker. Drives the WORLDGEN
                        // atomic on Play; same combo also affects
                        // Host server. Persists across menu visits
                        // via the chosen variant stored as the
                        // current atomic value.
                        let mut wg = WorldGen::current();
                        ui.horizontal(|ui| {
                            ui.label("World:");
                            egui::ComboBox::from_id_salt("world_pick")
                                .selected_text(format!("{wg:?}"))
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut wg,
                                        WorldGen::Classic,
                                        "Classic",
                                    );
                                    ui.selectable_value(
                                        &mut wg,
                                        WorldGen::Flat,
                                        "Flat (debug)",
                                    );
                                    ui.selectable_value(
                                        &mut wg,
                                        WorldGen::Islands,
                                        "Islands",
                                    );
                                    ui.selectable_value(
                                        &mut wg,
                                        WorldGen::Amplified,
                                        "Amplified",
                                    );
                                });
                        });
                        WORLDGEN.store(
                            wg as u8,
                            std::sync::atomic::Ordering::Relaxed,
                        );
                        // Worlds picker scaffolding (Phase B.7a).
                        // Final polish lands in Phase G.
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("Worlds")
                                .strong()
                                .size(15.0),
                        );
                        if let Some(slug) = &current_slug {
                            ui.label(
                                egui::RichText::new(format!(
                                    "Active slot: {slug}"
                                ))
                                .small()
                                .italics(),
                            );
                        }
                        ui.horizontal(|ui| {
                            ui.label("Name:");
                            ui.text_edit_singleline(new_world_name);
                            if ui.button("New").clicked() {
                                action_new_world = true;
                            }
                        });
                        // Existing slots list. Compact rows so the
                        // main menu doesn't grow unbounded for users
                        // with many saves; the proper polished
                        // picker lands in Phase G.
                        egui::ScrollArea::vertical()
                            .max_height(140.0)
                            .show(ui, |ui| {
                                if worlds_list.is_empty() {
                                    ui.label(
                                        egui::RichText::new(
                                            "(no saved worlds yet)",
                                        )
                                        .italics(),
                                    );
                                }
                                for w in &worlds_list {
                                    ui.horizontal(|ui| {
                                        let label = format!(
                                            "{}  -  {} (seed {})",
                                            w.name, w.worldgen, w.seed
                                        );
                                        ui.label(label);
                                        if ui.button("Load").clicked() {
                                            action_load_world = Some(w.slug.clone());
                                        }
                                        if ui.button("Delete").clicked() {
                                            action_delete_world = Some(w.slug.clone());
                                        }
                                    });
                                }
                            });
                        ui.add_space(8.0);
                        if ui.button("Play offline").clicked() {
                            action_start = true;
                        }
                        if ui.button("Host server...").clicked() {
                            action_to_host = true;
                        }
                        if ui.button("Direct connect...").clicked() {
                            action_to_direct = true;
                        }
                        if ui.button("Browse servers...").clicked() {
                            action_to_browser = true;
                        }
                        ui.add_space(10.0);
                        if ui.button("Settings").clicked() {
                            action_menu_to_settings = true;
                        }
                        if ui.button("Quit").clicked() {
                            action_quit = true;
                        }
                        ui.add_space(20.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "Playing as: {}",
                                player_name_buf
                            ))
                            .color(egui::Color32::from_rgb(180, 200, 200))
                            .italics(),
                        );
                        ui.label(
                            egui::RichText::new(format!(
                                "Seed: {}",
                                world_seed()
                            ))
                            .color(egui::Color32::from_rgb(150, 180, 180))
                            .italics()
                            .small(),
                        );
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.label("Master server:");
                            ui.text_edit_singleline(master_addr);
                        });
                    });
                });
            }
            AppState::DirectConnect => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.heading("Direct connect");
                        ui.add_space(10.0);
                        ui.label("Enter host:port of the game server:");
                        ui.text_edit_singleline(direct_buf);
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button("Join").clicked() {
                                action_join_direct = true;
                            }
                            if ui.button("Back").clicked() {
                                action_main_menu = true;
                            }
                        });
                    });
                });
            }
            AppState::ServerBrowser => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.heading("Server browser");
                    ui.horizontal(|ui| {
                        ui.label("Master:");
                        ui.text_edit_singleline(master_addr);
                        if ui.button("Refresh").clicked() {
                            action_browser_refresh = true;
                        }
                        if ui.button("Back").clicked() {
                            action_main_menu = true;
                        }
                    });
                    // Diagnostic status banner so the user knows
                    // whether the last query reached the master.
                    ui.horizontal(|ui| {
                        let age = browser_last_refresh
                            .map(|t| format!("{:.0}s ago", t.elapsed().as_secs_f32()));
                        let status = if browser_querying {
                            "[...] querying master".to_string()
                        } else {
                            match (browser_error.as_deref(), age.as_deref()) {
                                (Some(err), _) => format!("[ERR] {err}"),
                                (None, Some(a)) => format!(
                                    "[OK] {} server(s) - refreshed {a}",
                                    browser_servers.len()
                                ),
                                (None, None) => "[idle] press Refresh".to_string(),
                            }
                        };
                        let color = if browser_querying {
                            egui::Color32::from_rgb(140, 180, 220)
                        } else if browser_error.is_some() {
                            egui::Color32::from_rgb(220, 100, 100)
                        } else if browser_servers.is_empty() {
                            egui::Color32::from_rgb(220, 200, 100)
                        } else {
                            egui::Color32::from_rgb(140, 220, 140)
                        };
                        ui.colored_label(color, status);
                    });
                    ui.separator();
                    if browser_servers.is_empty() && browser_error.is_none() {
                        ui.label(
                            "(no servers - host one from the menu or wait for someone to announce)"
                        );
                    }
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        for s in browser_servers {
                            ui.horizontal(|ui| {
                                if ui.button("Join").clicked() {
                                    action_join_browser = Some(s.addr.clone());
                                }
                                ui.label(format!(
                                    "{}  -  {} ({}/{}{})",
                                    s.addr,
                                    s.name,
                                    s.players,
                                    s.max_players,
                                    if s.mode.is_empty() {
                                        String::new()
                                    } else {
                                        format!(", {}", s.mode)
                                    }
                                ));
                            });
                        }
                    });
                });
            }
            AppState::HostSetup => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(60.0);
                        ui.heading("Host server");
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            ui.label("Display name:");
                            ui.text_edit_singleline(host_name);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Bind port:");
                            ui.text_edit_singleline(host_port);
                        });
                        ui.checkbox(host_announce, "Announce on master server");
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button("Start hosting").clicked() {
                                action_host_start = true;
                            }
                            if ui.button("Back").clicked() {
                                action_main_menu = true;
                            }
                        });
                    });
                });
            }
            AppState::Paused => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(80.0);
                        ui.heading("Paused");
                        ui.add_space(20.0);
                        if ui.button("Resume").clicked() {
                            action_resume = true;
                        }
                        if ui.button("Settings").clicked() {
                            action_pause_to_settings = true;
                        }
                        if ui.button("Main Menu").clicked() {
                            action_main_menu = true;
                        }
                        ui.add_space(20.0);
                        ui.label(
                            egui::RichText::new(format!(
                                "Seed: {}",
                                world_seed()
                            ))
                            .color(egui::Color32::from_rgb(150, 180, 180))
                            .small()
                            .italics(),
                        );
                    });
                });
            }
            AppState::Settings { .. } => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.heading("Settings");
                    ui.horizontal(|ui| {
                        ui.label("Player name:");
                        ui.text_edit_singleline(player_name_buf);
                    });
                    ui.add(
                        egui::Slider::new(&mut settings_edit.mouse_sensitivity, 0.1..=2.0)
                            .text("Mouse sensitivity"),
                    );
                    ui.add(
                        egui::Slider::new(&mut settings_edit.view_distance, 2..=12)
                            .text("Render distance (chunks)"),
                    );
                    ui.checkbox(&mut settings_edit.show_fps, "Show FPS");
                    ui.checkbox(
                        &mut settings_edit.lock_cursor,
                        "Lock cursor in window (Play)",
                    );
                    ui.checkbox(&mut settings_edit.hide_cursor, "Hide cursor (Play)");
                    ui.checkbox(&mut settings_edit.head_bob, "Camera head bob");
                    ui.checkbox(
                        &mut settings_edit.invert_mouse_y,
                        "Invert mouse Y",
                    );
                    ui.checkbox(
                        &mut settings_edit.invert_wheel,
                        "Invert hotbar scroll",
                    );
                    ui.separator();
                    if ui.button("Back").clicked() {
                        action_back_settings = true;
                    }
                });
            }
            AppState::Playing => {
                if hud_visible {
                egui::Area::new(egui::Id::new("voxel_hud"))
                    .anchor(egui::Align2::LEFT_TOP, [12.0, 12.0])
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.label("Mouse aim, WASD move, Space jump");
                            ui.label("Shift sneak, Ctrl sprint");
                            ui.label("LMB break, RMB place, E inventory, F eat, Esc menu");
                            ui.label("T chat, Tab playerlist, Q drop, Z undo, F1 hide HUD, F3 debug, F5 camera");
                            // Day clock - map [0..DAY_LENGTH_S) onto
                            // 24h so the player has a sense of where
                            // the sun is.
                            let frac = (day_time / DAY_LENGTH_S).rem_euclid(1.0);
                            let hours_total = frac * 24.0;
                            let hh = hours_total.floor() as i32;
                            let mm = ((hours_total - hh as f32) * 60.0) as i32;
                            // Cardinal direction the player is
                            // facing. Yaw=0 -> east (+X); rotate
                            // by quadrants for the standard
                            // N/E/S/W readout.
                            let yaw_norm = dbg_yaw.rem_euclid(std::f32::consts::TAU);
                            let octant = (yaw_norm / std::f32::consts::FRAC_PI_4)
                                .round() as i32
                                & 7;
                            let dir = match octant {
                                0 => "E",
                                1 => "NE",
                                2 => "N",
                                3 => "NW",
                                4 => "W",
                                5 => "SW",
                                6 => "S",
                                _ => "SE",
                            };
                            ui.label(format!(
                                "Time: {hh:02}:{mm:02}    Kills: {kill_count}    Facing: {dir}    Biome: {biome_name}{}",
                                match weather_label {
                                    Some(w) => format!("    Weather: {w}"),
                                    None => String::new(),
                                }
                            ));
                            ui.label(format!(
                                "XYZ: {:.0}, {:.0}, {:.0}",
                                dbg_player.x,
                                dbg_player.y,
                                dbg_player.z
                            ));
                            if let Some(tool) = held_tool_label {
                                ui.label(
                                    egui::RichText::new(format!(
                                        "Held: {tool}"
                                    ))
                                    .color(egui::Color32::from_rgb(
                                        200, 230, 255,
                                    )),
                                );
                            }
                            // Spawn-direction hint: distance to
                            // the configured respawn point + the
                            // approximate compass bearing toward
                            // it. Hidden when standing on spawn.
                            let sp = spawn_xz;
                            let dxs = sp.0 - dbg_player.x;
                            let dzs = sp.1 - dbg_player.z;
                            let dist_sp = (dxs * dxs + dzs * dzs).sqrt();
                            if dist_sp > 4.0 {
                                let yaw_to =
                                    (-dzs).atan2(dxs).rem_euclid(
                                        std::f32::consts::TAU,
                                    );
                                let oct = (yaw_to
                                    / std::f32::consts::FRAC_PI_4)
                                    .round() as i32
                                    & 7;
                                let dir_sp = match oct {
                                    0 => "E",
                                    1 => "NE",
                                    2 => "N",
                                    3 => "NW",
                                    4 => "W",
                                    5 => "SW",
                                    6 => "S",
                                    _ => "SE",
                                };
                                ui.label(format!(
                                    "Spawn: {dir_sp}  {dist_sp:.0}m"
                                ));
                            }
                            // Health row: 10 hearts of 2 HP each.
                            // A heart is full if `player_health` >=
                            // 2 * (i + 1), half if at the half-step,
                            // empty otherwise.
                            ui.horizontal(|ui| {
                                ui.label("HP:");
                                for i in 0..10 {
                                    let need_full = 2.0 * (i as f32 + 1.0);
                                    let need_half = need_full - 1.0;
                                    let glyph = if player_health >= need_full {
                                        "\u{2665}"
                                    } else if player_health >= need_half {
                                        "\u{2661}"
                                    } else {
                                        " "
                                    };
                                    ui.colored_label(
                                        egui::Color32::from_rgb(220, 50, 50),
                                        glyph,
                                    );
                                }
                            });
                            // Stamina bar. Width tracks the
                            // current fraction; tint shifts from
                            // green at full down to orange at
                            // low. Hidden when sprinting is
                            // impossible (full + not in use).
                            if stamina_frac < 0.999 || self.sprinting {
                                ui.horizontal(|ui| {
                                    ui.label("Stamina:");
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(120.0, 8.0),
                                        egui::Sense::hover(),
                                    );
                                    let painter = ui.painter();
                                    painter.rect_filled(
                                        rect,
                                        2.0,
                                        egui::Color32::from_rgba_unmultiplied(
                                            30, 30, 30, 220,
                                        ),
                                    );
                                    let fill = egui::Rect::from_min_size(
                                        rect.min,
                                        egui::vec2(
                                            rect.width() * stamina_frac,
                                            rect.height(),
                                        ),
                                    );
                                    let col = if stamina_frac > 0.5 {
                                        egui::Color32::from_rgb(100, 200, 80)
                                    } else if stamina_frac > 0.2 {
                                        egui::Color32::from_rgb(220, 180, 60)
                                    } else {
                                        egui::Color32::from_rgb(220, 90, 60)
                                    };
                                    painter.rect_filled(fill, 2.0, col);
                                });
                            }
                            // Air bar - only visible while the
                            // breath meter is below full. Drains
                            // submerged, refills on the surface.
                            // Tints from cyan -> orange -> red as
                            // the meter empties.
                            if air_frac < 0.999 {
                                ui.horizontal(|ui| {
                                    ui.label("Air:");
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(120.0, 8.0),
                                        egui::Sense::hover(),
                                    );
                                    let painter = ui.painter();
                                    painter.rect_filled(
                                        rect,
                                        2.0,
                                        egui::Color32::from_rgba_unmultiplied(
                                            30, 30, 30, 220,
                                        ),
                                    );
                                    let fill = egui::Rect::from_min_size(
                                        rect.min,
                                        egui::vec2(
                                            rect.width() * air_frac,
                                            rect.height(),
                                        ),
                                    );
                                    let col = if air_frac > 0.5 {
                                        egui::Color32::from_rgb(100, 180, 230)
                                    } else if air_frac > 0.2 {
                                        egui::Color32::from_rgb(220, 180, 60)
                                    } else {
                                        egui::Color32::from_rgb(220, 60, 60)
                                    };
                                    painter.rect_filled(fill, 2.0, col);
                                });
                            }
                            if show_fps {
                                ui.separator();
                                ui.label(format!("FPS: {fps:.0}"));
                            }
                            // Phase F.7: show the active dim so
                            // the player has feedback after a
                            // /dim or portal teleport. Cheap
                            // ASCII; no separator unless FPS is
                            // visible (keeps the HUD tidy).
                            if show_fps {
                                ui.separator();
                            }
                            ui.label(format!("Dim: {dim}"));
                        });
                    });
                }
                // Multiplayer in-game menu. Opens on Esc when a
                // net role is active. World keeps simulating
                // underneath (other peers must not see a frozen
                // ghost), so this is a thin overlay rather than a
                // pause state.
                if mp_menu_open
                    && let Some(role) = mp_role_label
                {
                    egui::Window::new("Game menu")
                        .open(&mut mp_menu_open_local)
                        .collapsible(false)
                        .resizable(false)
                        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                        .show(ctx, |ui| {
                            ui.label(format!("Status: {role}"));
                            ui.separator();
                            if ui
                                .add_sized([220.0, 32.0], egui::Button::new("Resume"))
                                .clicked()
                            {
                                action_mp_resume = true;
                            }
                            if ui
                                .add_sized(
                                    [220.0, 32.0],
                                    egui::Button::new("Disconnect to main menu"),
                                )
                                .clicked()
                            {
                                action_mp_disconnect = true;
                            }
                            if ui
                                .add_sized([220.0, 32.0], egui::Button::new("Quit"))
                                .clicked()
                            {
                                action_mp_quit = true;
                            }
                        });
                }
                // F3 debug panel: top-right corner, lots of state in
                // one place. Mirrors Minecraft's debug overlay.
                // F6 live debug panel: sliders + toggles for
                // common runtime tweaks. Lives in its own
                // resizable window so it's draggable.
                if dbg_panel_was_open {
                    egui::Window::new("Debug panel (F6)")
                        .open(&mut dbg_panel_open)
                        .collapsible(true)
                        .resizable(true)
                        .default_pos([20.0, 180.0])
                        .default_size([280.0, 320.0])
                        .show(ctx, |ui| {
                            // Time of day slider 0..24h.
                            let mut hours =
                                (*dbg_day_time / DAY_LENGTH_S) * 24.0;
                            if ui
                                .add(
                                    egui::Slider::new(&mut hours, 0.0..=24.0)
                                        .text("Time (h)")
                                        .step_by(0.01),
                                )
                                .changed()
                            {
                                *dbg_day_time = (hours / 24.0)
                                    * DAY_LENGTH_S;
                            }
                            ui.checkbox(dbg_freeze_time, "Freeze time");
                            // Weather dropdown.
                            ui.horizontal(|ui| {
                                ui.label("Weather:");
                                ui.selectable_value(
                                    dbg_weather,
                                    0,
                                    "Clear",
                                );
                                ui.selectable_value(
                                    dbg_weather,
                                    1,
                                    "Rain",
                                );
                                ui.selectable_value(
                                    dbg_weather,
                                    2,
                                    "Snow",
                                );
                            });
                            // FOV slider in degrees.
                            let mut fov_deg = dbg_fov_rad.to_degrees();
                            if ui
                                .add(
                                    egui::Slider::new(
                                        &mut fov_deg,
                                        30.0..=110.0,
                                    )
                                    .text("FOV"),
                                )
                                .changed()
                            {
                                *dbg_fov_rad = fov_deg.to_radians();
                            }
                            ui.add(
                                egui::Slider::new(
                                    &mut settings_edit.view_distance,
                                    2..=12,
                                )
                                .text("Render distance"),
                            );
                            ui.add(
                                egui::Slider::new(
                                    dbg_mob_cap,
                                    0..=64,
                                )
                                .text("Mob cap"),
                            );
                            ui.add(
                                egui::Slider::new(
                                    dbg_snow_rate,
                                    0.05..=2.0,
                                )
                                .text("Snow density (s/layer)"),
                            );
                            ui.checkbox(dbg_fly_mode, "Fly mode");
                            ui.checkbox(dbg_god_mode, "Godmode");
                            ui.checkbox(
                                dbg_liquid_bias,
                                "Liquid drop bias (pathfinding)",
                            );
                            ui.separator();
                            ui.label(
                                "Hotkeys: F3 stats, F4 weather, F5 camera, G chunk grid",
                            );
                        });
                }
                if show_debug {
                    egui::Area::new(egui::Id::new("voxel_debug"))
                        .anchor(egui::Align2::RIGHT_TOP, [-12.0, 12.0])
                        .show(ctx, |ui| {
                            egui::Frame::popup(ui.style()).show(ui, |ui| {
                                ui.label("[F3] Debug");
                                ui.separator();
                                ui.label(format!(
                                    "pos: {:.2}, {:.2}, {:.2}",
                                    dbg_player.x, dbg_player.y, dbg_player.z
                                ));
                                ui.label(format!(
                                    "look: yaw {:.2} pitch {:.2}",
                                    dbg_yaw, dbg_pitch
                                ));
                                ui.label(format!(
                                    "grounded: {}",
                                    if dbg_grounded { "yes" } else { "no" }
                                ));
                                ui.label(format!("chunk meshes: {dbg_chunks}"));
                                ui.label(format!("pending chunks: {dbg_pending}"));
                                ui.label(format!("mobs (remote): {dbg_mobs}"));
                                ui.label(format!("particles: {dbg_particles}"));
                                ui.label(format!(
                                    "camera: {dbg_cam}{}",
                                    if dbg_fly { "  fly" } else { "" }
                                ));
                                ui.label(format!("fps: {fps:.0}"));
                                ui.label(format!("net: {dbg_net}"));
                                let h = dbg_playtime / 3600;
                                let m = (dbg_playtime % 3600) / 60;
                                let s = dbg_playtime % 60;
                                ui.label(format!("session: {h:02}:{m:02}:{s:02}"));
                                ui.label(format!("seed: {}", world_seed()));
                            });
                        });
                }
                // Bottom hotbar - 9 normal slots. Mining is driven
                // by LMB regardless of what's held; the held tool
                // only affects mining speed + drops. Highlighted
                // slot is the active selection (driven by mouse
                // wheel). Clicks here change the active slot;
                // drag&drop is wired through the inventory window.
                if hud_visible {
                egui::Area::new(egui::Id::new("voxel_hotbar"))
                    .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -16.0])
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.horizontal(|ui| {
                                    for (i, slot) in hotbar_snapshot.iter().enumerate() {
                                        let resp = slot_with_dnd(
                                            ui,
                                            *slot,
                                            atlas_tex_id,
                                            SlotKind::Inv(i),
                                            i == selected_slot,
                                            inventory_open,
                                            &mut inv_actions,
                                        );
                                        if resp.clicked() {
                                            inv_actions.push(InvAction::SelectHotbar(i));
                                        }
                                    }
                                });
                                // Row of digit hints under each
                                // slot. Highlights the active slot
                                // so the player can map their key
                                // press to the selected stack at a
                                // glance.
                                ui.horizontal(|ui| {
                                    for i in 0..HOTBAR_SLOTS {
                                        let label = if i < 9 {
                                            format!("{}", i + 1)
                                        } else {
                                            String::new()
                                        };
                                        let color = if i == selected_slot {
                                            egui::Color32::from_rgb(255, 220, 100)
                                        } else {
                                            egui::Color32::from_rgb(170, 170, 170)
                                        };
                                        ui.add_sized(
                                            [38.0, 14.0],
                                            egui::Label::new(
                                                egui::RichText::new(label)
                                                    .color(color)
                                                    .monospace(),
                                            ),
                                        );
                                    }
                                });
                            });
                        });
                    });
                }

                // Inventory overlay: 3 extra rows of slots + a hotbar
                // mirror + a 2x2 crafting grid + a 1-slot output.
                // Only drawn when the player has the menu open. The
                // Window's title bar is the only drag handle (egui
                // default behaviour with `.resizable(false)`).
                if inventory_open {
                    let screen = ctx.content_rect();
                    egui::Window::new("Inventory")
                        .open(&mut inv_open_local)
                        .collapsible(false)
                        .resizable(false)
                        .default_pos([
                            screen.center().x - 220.0,
                            screen.center().y - 220.0,
                        ])
                        .default_size([440.0, 420.0])
                        .show(ctx, |ui| {
                            ui.label("E to close - drag stacks between slots");
                            ui.separator();
                            // Storage rows (rows 1..INVENTORY_ROWS).
                            for row in 0..(INVENTORY_ROWS - 1) {
                                ui.horizontal(|ui| {
                                    for col in 0..INVENTORY_COLS {
                                        let snap_idx = row * INVENTORY_COLS + col;
                                        let inv_idx = HOTBAR_SLOTS + snap_idx;
                                        let slot = inventory_snapshot
                                            .get(snap_idx)
                                            .copied()
                                            .unwrap_or(None);
                                        slot_with_dnd(
                                            ui,
                                            slot,
                                            atlas_tex_id,
                                            SlotKind::Inv(inv_idx),
                                            false,
                                            true,
                                            &mut inv_actions,
                                        );
                                    }
                                });
                            }
                            ui.add_space(8.0);
                            // Hotbar row reflected inside the window
                            // so the player can shuffle stacks
                            // between storage and the hotbar without
                            // closing the menu (matches Minecraft).
                            ui.horizontal(|ui| {
                                for (i, slot) in hotbar_snapshot.iter().enumerate() {
                                    let resp = slot_with_dnd(
                                        ui,
                                        *slot,
                                        atlas_tex_id,
                                        SlotKind::Inv(i),
                                        i == selected_slot,
                                        true,
                                        &mut inv_actions,
                                    );
                                    if resp.clicked() {
                                        inv_actions.push(InvAction::SelectHotbar(i));
                                    }
                                }
                            });
                            ui.separator();
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label("Craft (2x2)");
                                    for row in 0..2 {
                                        ui.horizontal(|ui| {
                                            for col in 0..2 {
                                                let idx = row * 2 + col;
                                                slot_with_dnd(
                                                    ui,
                                                    craft_snapshot[idx],
                                                    atlas_tex_id,
                                                    SlotKind::Craft(idx),
                                                    false,
                                                    true,
                                                    &mut inv_actions,
                                                );
                                            }
                                        });
                                    }
                                });
                                ui.vertical(|ui| {
                                    ui.add_space(20.0);
                                    ui.label(" ->");
                                });
                                ui.vertical(|ui| {
                                    ui.label("Output");
                                    let resp = slot_with_dnd(
                                        ui,
                                        craft_output_snapshot,
                                        atlas_tex_id,
                                        SlotKind::CraftOut,
                                        false,
                                        true,
                                        &mut inv_actions,
                                    );
                                    if let Some(name) = craft_recipe_name {
                                        resp.on_hover_text(name);
                                    }
                                });
                            });
                            // Carry preview is unnecessary - egui's
                            // dnd_drag_source paints the dragged
                            // stack at the cursor while held. The
                            // recipe book lives in its own window
                            // beside this one (see below) so it can
                            // grow / scroll without pushing the
                            // crafting grid off-screen.
                        });
                    // Recipe book as a separate beside-inventory
                    // window. Resizable + scrollable so it can fit
                    // the full recipe table even on a small display
                    // without cramping the Inventory layout.
                    let rb_default = [
                        screen.center().x + 230.0,
                        screen.center().y - 220.0,
                    ];
                    egui::Window::new("Recipe book")
                        .collapsible(true)
                        .resizable(true)
                        .default_pos(rb_default)
                        .default_size([320.0, 420.0])
                        .min_width(280.0)
                        .min_height(200.0)
                        .show(ctx, |ui| {
                            ui.label(
                                "Drop blocks into the 2x2 grid to craft. \
                                 Hold Shift for half-stacks, Ctrl/Cmd for one.",
                            );
                            ui.separator();
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    for (ri, r) in
                                        RECIPES.iter().enumerate()
                                    {
                                        ui.horizontal(|ui| {
                                            for (bid, n) in r.inputs {
                                                let bt = block_type(*bid);
                                                let icon = self
                                                    .registry
                                                    .get(BlockId(*bid))
                                                    .map(|d| {
                                                        (
                                                            d.color,
                                                            *n,
                                                            atlas_uv_rect(
                                                                BlockId(*bid),
                                                                [0, 1, 0],
                                                            ),
                                                            bt.map(|t| t.name)
                                                                .unwrap_or("?"),
                                                        )
                                                    });
                                                draw_slot_widget(
                                                    ui,
                                                    icon,
                                                    atlas_tex_id,
                                                    false,
                                                    egui::Sense::hover(),
                                                );
                                            }
                                            ui.label(" -> ");
                                            let out_bt = block_type(r.output.0);
                                            let out_icon = self
                                                .registry
                                                .get(BlockId(r.output.0))
                                                .map(|d| {
                                                    (
                                                        d.color,
                                                        r.output.1,
                                                        atlas_uv_rect(
                                                            BlockId(r.output.0),
                                                            [0, 1, 0],
                                                        ),
                                                        out_bt
                                                            .map(|t| t.name)
                                                            .unwrap_or("?"),
                                                    )
                                                });
                                            let out_resp = draw_slot_widget(
                                                ui,
                                                out_icon,
                                                atlas_tex_id,
                                                false,
                                                egui::Sense::click(),
                                            )
                                            .on_hover_text(format!(
                                                "{} - click to fill craft grid",
                                                r.name
                                            ));
                                            if out_resp.clicked() {
                                                prefill_recipe_idx = Some(ri);
                                            }
                                            ui.label(r.name);
                                        });
                                    }
                                });
                        });
                }

                // Chest UI panel. Shows a 27-slot chest grid on
                // top + the player's 36-slot inventory below.
                // Click semantics live in `container::ui::draw_grid`:
                // LMB = take/place, RMB = take half / place one,
                // Shift+LMB = transfer to partner. The cursor-
                // held stack is rendered as a small floating
                // indicator near the cursor; the actual mutation
                // happens after the egui closure exits so the
                // borrow checker is happy.
                if let Some(chest) = &chest_snapshot {
                    let screen = ctx.content_rect();
                    egui::Window::new("Chest")
                        .open(&mut chest_open_local)
                        .collapsible(false)
                        .resizable(false)
                        .default_pos([
                            screen.center().x - 220.0,
                            screen.center().y - 240.0,
                        ])
                        .default_size([440.0, 460.0])
                        .show(ctx, |ui| {
                            ui.label("ESC to close - LMB take/place, RMB half/one, Shift+LMB transfer");
                            if let Some((id, n)) = chest_held_local {
                                ui.label(format!(
                                    "Holding: item {} x{}",
                                    crate::item::item_display_name(id),
                                    n
                                ));
                            }
                            ui.separator();
                            ui.label("Chest (27)");
                            let chest_cfg = container::ui::GridConfig {
                                cols: 9,
                                rows: 3,
                                tile_px: 36.0,
                                title: None,
                            };
                            let actions = container::ui::draw_grid(
                                ui,
                                &chest_cfg,
                                chest,
                                chest_held_local,
                                atlas_tex_id,
                            );
                            for a in actions {
                                chest_grid_actions.push((false, a));
                            }
                            ui.separator();
                            ui.label("Inventory (36)");
                            // Inventory rendered through an
                            // on-the-fly Container view so the
                            // generic widget can drive clicks on
                            // it too. The on-screen contents come
                            // straight from `inv_plain_snapshot`,
                            // mirroring the host's authoritative
                            // inventory at the start of the frame.
                            let mut inv_view =
                                container::Container::with_size(
                                    inv_plain_snapshot.len(),
                                );
                            for (i, s) in inv_plain_snapshot.iter().enumerate() {
                                inv_view.set_slot(i, *s);
                            }
                            let inv_cfg = container::ui::GridConfig {
                                cols: 9,
                                rows: 4,
                                tile_px: 36.0,
                                title: None,
                            };
                            let inv_actions_g = container::ui::draw_grid(
                                ui,
                                &inv_cfg,
                                &inv_view,
                                chest_held_local,
                                atlas_tex_id,
                            );
                            for a in inv_actions_g {
                                chest_grid_actions.push((true, a));
                            }
                        });
                }

                // Furnace UI panel. Three slots laid out as
                // [input] - flame icon - [fuel] - cook arrow -
                // [output]. The flame bar shows fuel_left /
                // fuel_full; the arrow shows cook_progress /
                // FURNACE_COOK_MS_PER_OUTPUT. The player inv grid
                // sits below, mirroring the chest panel layout.
                if let Some(furnace) = &furnace_snapshot {
                    let screen = ctx.content_rect();
                    egui::Window::new("Furnace")
                        .open(&mut furnace_open_local)
                        .collapsible(false)
                        .resizable(false)
                        .default_pos([
                            screen.center().x - 220.0,
                            screen.center().y - 240.0,
                        ])
                        .default_size([440.0, 380.0])
                        .show(ctx, |ui| {
                            ui.label("ESC to close - LMB take/place, RMB half/one, Shift+LMB transfer");
                            if let Some((id, n)) = furnace_held_local {
                                ui.label(format!(
                                    "Holding: item {} x{}",
                                    crate::item::item_display_name(id),
                                    n,
                                ));
                            }
                            ui.separator();
                            // Top row: input slot + fuel slot
                            // + flame icon + cook arrow + output
                            // slot. Render the three slots as
                            // single-slot 1x1 grids so we can
                            // intersperse the bars.
                            ui.horizontal(|ui| {
                                ui.label("Input");
                                let cfg_input = container::ui::GridConfig {
                                    cols: 1, rows: 1, tile_px: 36.0, title: None,
                                };
                                let mut single = container::Container::with_size(1);
                                single.set_slot(0, furnace.slot(0));
                                let acts = container::ui::draw_grid(
                                    ui, &cfg_input, &single, furnace_held_local, atlas_tex_id,
                                );
                                for a in acts {
                                    // Remap idx 0 -> furnace
                                    // slot 0 (input).
                                    furnace_grid_actions.push((false, remap_action(a, 0)));
                                }
                                ui.label(" ");
                                // Fuel bar. Vertical filled rect
                                // proportional to fuel_left_ms /
                                // fuel_full_ms.
                                let fuel_frac = if furnace_runtime_snapshot.fuel_full_ms > 0 {
                                    furnace_runtime_snapshot.fuel_left_ms as f32
                                        / furnace_runtime_snapshot.fuel_full_ms as f32
                                } else {
                                    0.0
                                };
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(16.0, 36.0),
                                    egui::Sense::hover(),
                                );
                                let painter = ui.painter_at(rect);
                                painter.rect_filled(rect, 2.0, egui::Color32::from_gray(40));
                                let fill_h = rect.height() * fuel_frac.clamp(0.0, 1.0);
                                let fill_rect = egui::Rect::from_min_max(
                                    egui::pos2(rect.min.x, rect.max.y - fill_h),
                                    rect.max,
                                );
                                painter.rect_filled(
                                    fill_rect,
                                    2.0,
                                    egui::Color32::from_rgb(220, 110, 30),
                                );
                                ui.label("Fuel");
                                let cfg_fuel = container::ui::GridConfig {
                                    cols: 1, rows: 1, tile_px: 36.0, title: None,
                                };
                                let mut single_f = container::Container::with_size(1);
                                single_f.set_slot(0, furnace.slot(1));
                                let acts_f = container::ui::draw_grid(
                                    ui, &cfg_fuel, &single_f, furnace_held_local, atlas_tex_id,
                                );
                                for a in acts_f {
                                    furnace_grid_actions.push((false, remap_action(a, 1)));
                                }
                                // Cook arrow. Horizontal filled
                                // rect: cook_progress_ms /
                                // FURNACE_COOK_MS_PER_OUTPUT.
                                let cook_frac = furnace_runtime_snapshot.cook_progress_ms as f32
                                    / block::furnace::FURNACE_COOK_MS_PER_OUTPUT as f32;
                                let (arect, _) = ui.allocate_exact_size(
                                    egui::vec2(48.0, 16.0),
                                    egui::Sense::hover(),
                                );
                                let apainter = ui.painter_at(arect);
                                apainter.rect_filled(arect, 2.0, egui::Color32::from_gray(40));
                                let fill_w = arect.width() * cook_frac.clamp(0.0, 1.0);
                                let afill_rect = egui::Rect::from_min_max(
                                    arect.min,
                                    egui::pos2(arect.min.x + fill_w, arect.max.y),
                                );
                                apainter.rect_filled(
                                    afill_rect,
                                    2.0,
                                    egui::Color32::from_rgb(220, 200, 70),
                                );
                                ui.label("Out");
                                let cfg_out = container::ui::GridConfig {
                                    cols: 1, rows: 1, tile_px: 36.0, title: None,
                                };
                                let mut single_o = container::Container::with_size(1);
                                single_o.set_slot(0, furnace.slot(2));
                                let acts_o = container::ui::draw_grid(
                                    ui, &cfg_out, &single_o, furnace_held_local, atlas_tex_id,
                                );
                                for a in acts_o {
                                    furnace_grid_actions.push((false, remap_action(a, 2)));
                                }
                            });
                            ui.separator();
                            ui.label("Inventory (36)");
                            let mut inv_view = container::Container::with_size(
                                inv_plain_snapshot.len(),
                            );
                            for (i, s) in inv_plain_snapshot.iter().enumerate() {
                                inv_view.set_slot(i, *s);
                            }
                            let inv_cfg = container::ui::GridConfig {
                                cols: 9,
                                rows: 4,
                                tile_px: 36.0,
                                title: None,
                            };
                            let inv_actions_g = container::ui::draw_grid(
                                ui,
                                &inv_cfg,
                                &inv_view,
                                furnace_held_local,
                                atlas_tex_id,
                            );
                            for a in inv_actions_g {
                                furnace_grid_actions.push((true, a));
                            }
                        });
                }

                // Hurt-flash: full-screen red overlay that fades
                // out over ~0.5 s. Painted on the Foreground layer
                // so it sits above the world but below the
                // crosshair / inventory windows.
                if player_hurt_flash > 0.0 {
                    let alpha =
                        (player_hurt_flash / 0.5).clamp(0.0, 1.0).powf(1.3);
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_hurt_flash"),
                    ));
                    let rect = ctx.content_rect();
                    layer.rect_filled(
                        rect,
                        0.0,
                        egui::Color32::from_rgba_unmultiplied(
                            180,
                            0,
                            0,
                            (110.0 * alpha) as u8,
                        ),
                    );
                }
                // Underwater / lava view tint. Full-screen
                // composite under the foreground HUD so the
                // hotbar and bars stay readable. Pulled off the
                // submerged liquid's `submerged_tint` field so
                // every liquid kind paints in its own colour.
                if let Some(tint) = submerged_tint {
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_submerged_tint"),
                    ));
                    let rect = ctx.content_rect();
                    let r = (tint[0].clamp(0.0, 1.0) * 255.0) as u8;
                    let g = (tint[1].clamp(0.0, 1.0) * 255.0) as u8;
                    let b = (tint[2].clamp(0.0, 1.0) * 255.0) as u8;
                    let a = (tint[3].clamp(0.0, 1.0) * 255.0) as u8;
                    layer.rect_filled(
                        rect,
                        0.0,
                        egui::Color32::from_rgba_unmultiplied(r, g, b, a),
                    );
                }
                // Post-respawn invulnerability indicator: small
                // toast at the top centre while the timer is
                // positive. Pulses so the player knows the
                // window is finite.
                if hud_visible && invuln_remaining > 0.0 {
                    let alpha =
                        (0.6 + 0.4 * (invuln_remaining * 8.0).sin().abs())
                            .clamp(0.0, 1.0);
                    egui::Area::new(egui::Id::new("voxel_invuln"))
                        .anchor(egui::Align2::CENTER_TOP, [0.0, 60.0])
                        .show(ctx, |ui| {
                            egui::Frame::popup(ui.style())
                                .fill(egui::Color32::from_rgba_unmultiplied(
                                    20, 20, 60, 220,
                                ))
                                .show(ui, |ui| {
                                    let a = (255.0 * alpha) as u8;
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Invulnerable: {:.1}s",
                                            invuln_remaining
                                        ))
                                        .color(
                                            egui::Color32::from_rgba_unmultiplied(
                                                160, 200, 255, a,
                                            ),
                                        )
                                        .strong(),
                                    );
                                });
                        });
                }
                // Low-HP vignette: a pulsing red border that
                // ramps in below 6 HP and intensifies the closer
                // the player is to dying. The pulse uses day_time
                // as a continuous clock so we don't have to plumb
                // dt into the egui closure.
                if hud_visible && player_health > 0.0 && player_health < 6.0 {
                    let danger = ((6.0 - player_health) / 6.0).clamp(0.0, 1.0);
                    let pulse = 0.55
                        + 0.45
                            * (day_time * std::f32::consts::TAU * 1.6).sin()
                                .abs();
                    let base_alpha = (40.0 + 110.0 * danger) * pulse;
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_low_hp_vignette"),
                    ));
                    let rect = ctx.content_rect();
                    let thickness = (rect.width().min(rect.height()) * 0.22)
                        .max(40.0);
                    // Four edge bands so the centre stays clear -
                    // a darker corner read than a flat tint.
                    let top = egui::Rect::from_min_max(
                        rect.min,
                        egui::pos2(rect.max.x, rect.min.y + thickness),
                    );
                    let bottom = egui::Rect::from_min_max(
                        egui::pos2(rect.min.x, rect.max.y - thickness),
                        rect.max,
                    );
                    let left = egui::Rect::from_min_max(
                        rect.min,
                        egui::pos2(rect.min.x + thickness, rect.max.y),
                    );
                    let right = egui::Rect::from_min_max(
                        egui::pos2(rect.max.x - thickness, rect.min.y),
                        rect.max,
                    );
                    for r in [top, bottom, left, right] {
                        layer.rect_filled(
                            r,
                            0.0,
                            egui::Color32::from_rgba_unmultiplied(
                                160,
                                0,
                                0,
                                (base_alpha as u8).min(180),
                            ),
                        );
                    }
                }
                // Star field: dimly visible at dusk, brighter at
                // midnight, gone during the day. Each star sits at
                // a fixed direction on the celestial sphere; we
                // project that direction (treated as a far-away
                // world point relative to the player) through the
                // HUD view-projection so the field rotates with
                // the camera but doesn't slide with translation.
                //
                // `sky_blocked` was computed before the closure
                // against `self.world` since the closure has no
                // world capture.
                if night_factor > 0.0 && hud_visible && !sky_blocked {
                    const STAR_COUNT: u32 = 90;
                    const STAR_DIST: f32 = 250.0;
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Background,
                        egui::Id::new("voxel_stars"),
                    ));
                    let screen_rect = ctx.content_rect();
                    for i in 0..STAR_COUNT {
                        // Cheap deterministic direction: hash i to
                        // two angles + sample a hemisphere above
                        // the horizon (y > 0 in the celestial
                        // frame, rotated into world via day_time
                        // so the sky drifts).
                        let h1 =
                            (i.wrapping_mul(2654435761)) as f32 / u32::MAX as f32;
                        let h2 = ((i.wrapping_mul(40503))
                            .wrapping_add(0x9E37_79B9))
                            as f32
                            / u32::MAX as f32;
                        let azimuth = h1 * std::f32::consts::TAU;
                        let elev = h2 * std::f32::consts::FRAC_PI_2 * 0.95
                            + 0.05;
                        // Rotate the star around +X by the sun
                        // phase so the sky slowly drifts overnight.
                        let drift = phase * std::f32::consts::TAU;
                        let base = glam::Vec3::new(
                            elev.cos() * azimuth.cos(),
                            elev.sin(),
                            elev.cos() * azimuth.sin(),
                        );
                        let drifted = glam::Vec3::new(
                            base.x,
                            base.y * drift.cos() - base.z * drift.sin(),
                            base.y * drift.sin() + base.z * drift.cos(),
                        );
                        if drifted.y <= 0.05 {
                            continue;
                        }
                        let world = glam::Vec3::new(
                            hud_cam_pos.x + drifted.x * STAR_DIST,
                            hud_cam_pos.y + drifted.y * STAR_DIST,
                            hud_cam_pos.z + drifted.z * STAR_DIST,
                        );
                        let clip = hud_view_proj
                            * glam::Vec4::new(world.x, world.y, world.z, 1.0);
                        if clip.w <= 0.0 {
                            continue;
                        }
                        let ndc = glam::Vec3::new(
                            clip.x / clip.w,
                            clip.y / clip.w,
                            clip.z / clip.w,
                        );
                        if !(0.0..=1.0).contains(&ndc.z) {
                            continue;
                        }
                        let sx = (ndc.x * 0.5 + 0.5) * screen_rect.width()
                            + screen_rect.min.x;
                        let sy = (-ndc.y * 0.5 + 0.5) * screen_rect.height()
                            + screen_rect.min.y;
                        // Per-star brightness jitter so the field
                        // doesn't look uniform.
                        let brightness = 0.55 + (h2 * 3.7).fract() * 0.45;
                        let alpha = (220.0 * night_factor * brightness) as u8;
                        layer.circle_filled(
                            egui::pos2(sx, sy),
                            1.4,
                            egui::Color32::from_rgba_unmultiplied(
                                235, 240, 255, alpha,
                            ),
                        );
                    }
                }
                // Floating mob HP bars. Projected onto the screen
                // from each mob's world-space pos + a vertical
                // offset so the bar sits a bit above the head.
                // We hide bars at full HP + during the death fade.
                let screen = ctx.content_rect();
                if hud_visible && !mob_bars.is_empty() {
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_mob_hp"),
                    ));
                    for (pos, cur, max) in &mob_bars {
                        let world = glam::Vec3::new(pos[0], pos[1] + 1.55, pos[2]);
                        let clip = hud_view_proj
                            * glam::Vec4::new(world.x, world.y, world.z, 1.0);
                        if clip.w <= 0.0 {
                            continue;
                        }
                        let ndc = glam::Vec3::new(
                            clip.x / clip.w,
                            clip.y / clip.w,
                            clip.z / clip.w,
                        );
                        if !(0.0..=1.0).contains(&ndc.z) {
                            continue;
                        }
                        let sx = (ndc.x * 0.5 + 0.5) * screen.width()
                            + screen.min.x;
                        let sy = (-ndc.y * 0.5 + 0.5) * screen.height()
                            + screen.min.y;
                        // Scale bar width with depth so close
                        // enemies get a chunkier readout.
                        let depth_scale = (1.0 / clip.w * 30.0).clamp(0.25, 1.5);
                        let w = 50.0 * depth_scale;
                        let h = 5.0 * depth_scale;
                        let bg = egui::Rect::from_center_size(
                            egui::pos2(sx, sy),
                            egui::vec2(w, h),
                        );
                        layer.rect_filled(
                            bg,
                            1.0,
                            egui::Color32::from_rgba_unmultiplied(40, 0, 0, 200),
                        );
                        let frac = (*cur as f32 / *max as f32).clamp(0.0, 1.0);
                        let fill = egui::Rect::from_min_size(
                            bg.min,
                            egui::vec2(bg.width() * frac, bg.height()),
                        );
                        let bar_col = if frac > 0.5 {
                            egui::Color32::from_rgb(100, 200, 80)
                        } else if frac > 0.25 {
                            egui::Color32::from_rgb(220, 180, 60)
                        } else {
                            egui::Color32::from_rgb(220, 70, 60)
                        };
                        layer.rect_filled(fill, 1.0, bar_col);
                        layer.rect_stroke(
                            bg,
                            1.0,
                            egui::Stroke::new(
                                1.0,
                                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 200),
                            ),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
                // Floating nameplates above remote players. Same
                // projection trick as the mob HP bars; rendered
                // slightly higher than a sheep bar so it sits
                // clearly above the head cube. Hidden when the
                // player has toggled the HUD off (F1).
                if hud_visible && !nameplates.is_empty() {
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_nameplates"),
                    ));
                    for (pos, name, color) in &nameplates {
                        let world = glam::Vec3::new(pos[0], pos[1] + 0.55, pos[2]);
                        let clip = hud_view_proj
                            * glam::Vec4::new(world.x, world.y, world.z, 1.0);
                        if clip.w <= 0.0 {
                            continue;
                        }
                        let ndc = glam::Vec3::new(
                            clip.x / clip.w,
                            clip.y / clip.w,
                            clip.z / clip.w,
                        );
                        if !(0.0..=1.0).contains(&ndc.z) {
                            continue;
                        }
                        let sx = (ndc.x * 0.5 + 0.5) * screen.width()
                            + screen.min.x;
                        let sy = (-ndc.y * 0.5 + 0.5) * screen.height()
                            + screen.min.y;
                        // Fade text size with depth so far-away
                        // names don't dominate the screen.
                        let depth_scale = (1.0 / clip.w * 30.0).clamp(0.4, 1.4);
                        let font = egui::FontId::proportional(13.0 * depth_scale);
                        // Draw a 1px black outline by stamping the
                        // text four times before the foreground
                        // pass, then the main text on top.
                        for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                            layer.text(
                                egui::pos2(sx + dx, sy + dy),
                                egui::Align2::CENTER_BOTTOM,
                                name,
                                font.clone(),
                                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 220),
                            );
                        }
                        layer.text(
                            egui::pos2(sx, sy),
                            egui::Align2::CENTER_BOTTOM,
                            name,
                            font,
                            egui::Color32::from_rgb(color[0], color[1], color[2]),
                        );
                    }
                }
                // Tab playerlist overlay: centered modal listing
                // every known peer (incl. self) while Tab is held.
                if playerlist_visible && !playerlist.is_empty() {
                    egui::Area::new(egui::Id::new("voxel_playerlist"))
                        .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 40.0))
                        .order(egui::Order::Foreground)
                        .show(ctx, |ui| {
                            egui::Frame::popup(ui.style())
                                .fill(egui::Color32::from_rgba_unmultiplied(
                                    20, 20, 20, 210,
                                ))
                                .show(ui, |ui| {
                                    ui.set_min_width(180.0);
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Players ({})",
                                            playerlist.len()
                                        ))
                                        .strong()
                                        .color(egui::Color32::from_rgb(
                                            255, 220, 100,
                                        )),
                                    );
                                    ui.separator();
                                    for (name, is_self) in &playerlist {
                                        let mut txt = egui::RichText::new(name);
                                        if *is_self {
                                            txt = txt
                                                .color(egui::Color32::from_rgb(
                                                    160, 230, 160,
                                                ))
                                                .strong();
                                        } else {
                                            txt = txt.color(
                                                egui::Color32::from_rgb(
                                                    230, 230, 230,
                                                ),
                                            );
                                        }
                                        ui.label(txt);
                                    }
                                });
                        });
                }
                // Targeted-block label: name + mining progress
                // (when actively breaking the same voxel). Skipped
                // when the crosshair already shows a mob label
                // since both would overlap.
                if hud_visible
                    && targeted_mob_info.is_none()
                    && let Some((name, pct)) = &targeted_block_info
                {
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_block_label"),
                    ));
                    let center = screen.center();
                    let text = match pct {
                        Some(p) => format!("{name}  {p}%"),
                        None => name.to_string(),
                    };
                    layer.text(
                        egui::pos2(center.x, center.y + 22.0),
                        egui::Align2::CENTER_TOP,
                        text,
                        egui::FontId::proportional(13.0),
                        egui::Color32::from_rgb(220, 220, 220),
                    );
                }
                // Targeted-mob label: small text just under the
                // crosshair when the player is aiming at a mob.
                // Hidden when no mob is in line of sight.
                if hud_visible
                    && let Some((name, cur, max)) = targeted_mob_info
                {
                    let layer = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_mob_label"),
                    ));
                    let center = screen.center();
                    layer.text(
                        egui::pos2(center.x, center.y + 22.0),
                        egui::Align2::CENTER_TOP,
                        format!("{name}  {cur}/{max}"),
                        egui::FontId::proportional(14.0),
                        egui::Color32::from_rgb(240, 240, 240),
                    );
                }
                // Chunk-loading toast at the top centre while
                // a non-trivial batch is in flight. Cheaply
                // signals that the world is streaming in (e.g.
                // right after spawn or a /seed cycle).
                if hud_visible && dbg_pending > 8 {
                    egui::Area::new(egui::Id::new("voxel_loading"))
                        .anchor(egui::Align2::CENTER_TOP, [0.0, 14.0])
                        .show(ctx, |ui| {
                            egui::Frame::popup(ui.style())
                                .fill(egui::Color32::from_rgba_unmultiplied(
                                    20, 20, 20, 200,
                                ))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Generating world... ({dbg_pending} chunks pending)"
                                        ))
                                        .color(egui::Color32::from_rgb(
                                            255, 215, 120,
                                        )),
                                    );
                                });
                        });
                }
                // Pickup toasts on the right edge so the chat
                // log + hotbar stay readable on the left + bottom.
                if hud_visible && !pickup_snapshot.is_empty() {
                    egui::Area::new(egui::Id::new("voxel_pickup_toasts"))
                        .anchor(egui::Align2::RIGHT_BOTTOM, [-18.0, -120.0])
                        .show(ctx, |ui| {
                            for (alpha, line, color) in &pickup_snapshot {
                                let a = (255.0 * alpha) as u8;
                                let r = (color[0] * 255.0) as u8;
                                let g = (color[1] * 255.0) as u8;
                                let b = (color[2] * 255.0) as u8;
                                // Brighten the swatch a touch so
                                // darker block colours (e.g. coal)
                                // still pop against the night sky.
                                let r2 = (r as u16 * 5 / 4).min(255) as u8;
                                let g2 = (g as u16 * 5 / 4).min(255) as u8;
                                let b2 = (b as u16 * 5 / 4).min(255) as u8;
                                ui.horizontal(|ui| {
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(12.0, 12.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter().rect_filled(
                                        rect,
                                        2.0,
                                        egui::Color32::from_rgba_unmultiplied(
                                            r2, g2, b2, a,
                                        ),
                                    );
                                    ui.colored_label(
                                        egui::Color32::from_rgba_unmultiplied(
                                            245, 245, 245, a,
                                        ),
                                        line,
                                    );
                                });
                            }
                        });
                }
                // Chat: log of recent lines at bottom-left + an
                // input box when `chat_open`. Each log entry
                // fades out over ~10 s; messages older than that
                // are dropped after this pass.
                if hud_visible || chat_open {
                    egui::Area::new(egui::Id::new("voxel_chat"))
                        .anchor(egui::Align2::LEFT_BOTTOM, [12.0, -90.0])
                        .show(ctx, |ui| {
                            for (line, age, mention) in &chat_lines {
                                let alpha = ((10.0 - age) / 2.0).clamp(0.0, 1.0);
                                if alpha <= 0.0 {
                                    continue;
                                }
                                let a = (255.0 * alpha) as u8;
                                // Mentions of the local player
                                // name highlight in warm yellow
                                // so they stand out of the chat
                                // stream.
                                let col = if *mention {
                                    egui::Color32::from_rgba_unmultiplied(
                                        255, 220, 100, a,
                                    )
                                } else {
                                    egui::Color32::from_rgba_unmultiplied(
                                        240, 240, 240, a,
                                    )
                                };
                                ui.colored_label(col, line);
                            }
                            if chat_open {
                                // `add_sized` forces the widget to
                                // a hard 720x36 rect; the bare
                                // `TextEdit::desired_width` hint
                                // gets squashed by the parent
                                // `Area`'s auto-shrink so the box
                                // ended up tiny regardless.
                                let resp = ui.add_sized(
                                    [720.0, 36.0],
                                    egui::TextEdit::singleline(chat_buf)
                                        .font(egui::FontId::proportional(18.0))
                                        .margin(egui::Vec2::new(8.0, 6.0))
                                        .hint_text("press T to chat, Enter sends"),
                                );
                                resp.request_focus();
                                // History scrolling via Up/Down.
                                // Older entries are at the start
                                // of the Vec, newest at the end;
                                // Up walks backward, Down walks
                                // forward. Down past the newest
                                // entry clears the line so the
                                // player can compose fresh text.
                                let up_pressed = ui
                                    .input(|i| i.key_pressed(egui::Key::ArrowUp));
                                let down_pressed = ui
                                    .input(|i| i.key_pressed(egui::Key::ArrowDown));
                                if !chat_history_ref.is_empty() {
                                    if up_pressed {
                                        let new_idx = match *chat_history_idx {
                                            None => chat_history_ref.len() - 1,
                                            Some(i) => i.saturating_sub(1),
                                        };
                                        *chat_history_idx = Some(new_idx);
                                        *chat_buf =
                                            chat_history_ref[new_idx].clone();
                                    } else if down_pressed {
                                        match *chat_history_idx {
                                            Some(i)
                                                if i + 1 < chat_history_ref.len() =>
                                            {
                                                *chat_history_idx = Some(i + 1);
                                                *chat_buf =
                                                    chat_history_ref[i + 1]
                                                        .clone();
                                            }
                                            Some(_) => {
                                                *chat_history_idx = None;
                                                chat_buf.clear();
                                            }
                                            None => {}
                                        }
                                    }
                                }
                                // Enter sends + closes the chat.
                                // Don't gate on `lost_focus` -
                                // we force-focus the textbox each
                                // frame via `request_focus`, so
                                // focus never naturally leaves
                                // when the user hits Enter. The
                                // raw key check is enough.
                                if ui
                                    .input(|i| i.key_pressed(egui::Key::Enter))
                                {
                                    chat_send_now = true;
                                }
                            }
                        });
                }
                // Crosshair drawn directly on the egui screen layer.
                // Two thin segments through the screen centre.
                if hud_visible {
                    let center = screen.center();
                    let painter = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("voxel_crosshair"),
                    ));
                    let stroke =
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(240, 240, 240));
                    let half = 8.0_f32;
                    painter.line_segment(
                        [
                            egui::pos2(center.x - half, center.y),
                            egui::pos2(center.x + half, center.y),
                        ],
                        stroke,
                    );
                    painter.line_segment(
                        [
                            egui::pos2(center.x, center.y - half),
                            egui::pos2(center.x, center.y + half),
                        ],
                        stroke,
                    );
                }
            }
        });

        // Drop chat lines older than `CHAT_FADE_SECS`. Done after
        // the render pass so the egui closure had a chance to
        // display them one last frame.
        self.chat_log.retain(|(t, _)| {
            chat_now.saturating_duration_since(*t).as_secs_f32() < CHAT_FADE_SECS
        });
        // Send the chat line if the user pressed Enter.
        if chat_send_now {
            let text = self.chat_input.trim().to_string();
            self.chat_input.clear();
            self.chat_open = false;
            self.chat_history_idx = None;
            if !text.is_empty() {
                // Push to history, dedup adjacent repeats so
                // hitting Up + Enter twice doesn't double-fill
                // the log.
                if self.chat_history.last() != Some(&text) {
                    self.chat_history.push(text.clone());
                    if self.chat_history.len() > CHAT_HISTORY_MAX {
                        let drop = self.chat_history.len() - CHAT_HISTORY_MAX;
                        self.chat_history.drain(0..drop);
                    }
                }
                // Slash-command dispatch. Commands run locally
                // (no broadcast); everything else routes to chat
                // normally below.
                let handled_locally = chat::dispatch(self, &text);
                if !handled_locally {
                    match &self.net {
                        NetRole::Offline => {
                            // No-op offline: chat only makes sense
                            // in MP. Drop the input silently.
                        }
                        NetRole::Server { server, .. } => {
                            // Host prepends its own name + appends
                            // to the local log, then broadcasts so
                            // every client renders the same line.
                            let line =
                                format!("<{}> {text}", self.player_name);
                            self.chat_log
                                .push((std::time::Instant::now(), line.clone()));
                            if let Ok(buf) = bincode::serde::encode_to_vec(
                                &line,
                                bincode::config::standard(),
                            ) {
                                server.broadcast_bytes("vx.chat", buf);
                            }
                        }
                        NetRole::Client { client, .. } => {
                            // Client sends the raw text; the server
                            // tags it with the sender name before
                            // broadcasting back.
                            if let Ok(buf) = bincode::serde::encode_to_vec(
                                &text,
                                bincode::config::standard(),
                            ) {
                                let _ = client.send_bytes("vx.chat", buf);
                            }
                        }
                    }
                }
            }
        }
        // Apply inventory clicks captured during the egui pass. The
        // egui closure can't touch `self.inventory` directly (the
        // renderer already holds a `&mut self`), so we buffer the
        // intents and run them out here.
        //
        // Multiplayer client mode: every drag is forwarded to the
        // server as `vx.inv_act`. The local view is reconciled when
        // the server's authoritative snapshot lands. Hotbar selection
        // stays local for snappy feedback (it's purely cosmetic).
        let is_client = matches!(self.net, NetRole::Client { .. });
        for action in inv_actions {
            match action {
                InvAction::SelectHotbar(idx) => {
                    if idx < HOTBAR_SLOTS {
                        self.selected_slot = idx;
                    }
                }
                InvAction::Drag { from, to, split } => {
                    if is_client {
                        let msg = InvActionMsg {
                            from: from.into(),
                            to: to.into(),
                            split,
                        };
                        if let NetRole::Client {
                            client,
                            pending_inv_actions,
                            ..
                        } = &mut self.net
                        {
                            let cfg = bincode::config::standard();
                            if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                                let _ = client.send_bytes("vx.inv_act", buf);
                            }
                            pending_inv_actions.push(msg);
                        }
                    } else {
                        self.apply_inv_action(InvAction::Drag { from, to, split });
                    }
                }
            }
        }

        // If the debug panel mutated weather, broadcast the new
        // value to keep clients in sync.
        if self.weather != weather_before
            && let NetRole::Server { server, .. } = &mut self.net
            && let Ok(buf) = bincode::serde::encode_to_vec(
                self.weather,
                bincode::config::standard(),
            )
        {
            server.broadcast_bytes("vx.weather", buf);
        }
        // Reflect window-close-button clicks back to the state
        // flags they belong to.
        if dbg_panel_was_open && !dbg_panel_open {
            self.show_debug_panel = false;
        }
        if self.inventory_open && !inv_open_local {
            self.inventory_open = false;
        }
        if self.mp_menu_open && !mp_menu_open_local {
            self.mp_menu_open = false;
        }
        // Chest UI close-button reflected back. If the egui window
        // was closed via its X handle, drop the chest_open + held
        // state. Also drop the held cursor stack back to the
        // first free chest slot if the chest is still in range
        // (the simplest "don't lose items" recovery).
        if chest_open_pos.is_some() && !chest_open_local {
            self.close_chest_ui();
        }
        // Same close-button handling for the furnace UI.
        if furnace_open_pos.is_some() && !furnace_open_local {
            self.close_furnace_ui();
        }
        // Apply queued chest-UI actions. Each entry is
        // `(true_if_player_side, GridAction)`.
        if let Some(pos) = self.chest_open
            && !chest_grid_actions.is_empty()
        {
            if let Some(chest) = self.chests.get_mut(&pos) {
                // Mirror the actions onto a temp u16-shaped
                // player-inv buffer so the helpers (which expect
                // `[Option<(u16, u8)>]`) can mutate freely; then
                // copy back into the BlockId-shaped real inv.
                let mut inv_u16: Vec<Option<(u16, u8)>> = self
                    .inventory
                    .iter()
                    .map(|s| s.map(|(b, c)| (b.0, c)))
                    .collect();
                for (player_side, action) in chest_grid_actions {
                    if player_side {
                        block::chest::apply_player_action(
                            &mut inv_u16,
                            chest,
                            &mut chest_held_local,
                            action,
                        );
                    } else {
                        block::chest::apply_chest_action(
                            chest,
                            &mut inv_u16,
                            &mut chest_held_local,
                            action,
                        );
                    }
                }
                // Copy mutated inv back. Length matches because
                // we never resize.
                for (i, slot) in inv_u16.iter().enumerate() {
                    if i >= self.inventory.len() {
                        break;
                    }
                    self.inventory[i] = slot.map(|(b, c)| (BlockId(b), c));
                }
            }
            self.chest_held = chest_held_local;
        } else {
            // No actions but still surface the latest cursor
            // state for the next frame.
            self.chest_held = chest_held_local;
        }
        // Apply queued furnace-UI actions. Mirrors the chest path
        // but routes through `block::furnace::apply_furnace_action`
        // / `apply_player_action` so the accept-filter checks fire
        // before any slot mutation.
        if let Some(pos) = self.furnace_open
            && !furnace_grid_actions.is_empty()
        {
            if let Some(furnace) = self.furnaces.get_mut(&pos) {
                let mut inv_u16: Vec<Option<(u16, u8)>> = self
                    .inventory
                    .iter()
                    .map(|s| s.map(|(b, c)| (b.0, c)))
                    .collect();
                for (player_side, action) in furnace_grid_actions {
                    if player_side {
                        block::furnace::apply_player_action(
                            &mut inv_u16,
                            furnace,
                            &mut furnace_held_local,
                            action,
                        );
                    } else {
                        block::furnace::apply_furnace_action(
                            furnace,
                            &mut inv_u16,
                            &mut furnace_held_local,
                            action,
                        );
                    }
                }
                for (i, slot) in inv_u16.iter().enumerate() {
                    if i >= self.inventory.len() {
                        break;
                    }
                    self.inventory[i] = slot.map(|(b, c)| (BlockId(b), c));
                }
            }
            self.furnace_held = furnace_held_local;
        } else {
            self.furnace_held = furnace_held_local;
        }
        if action_start {
            self.state = AppState::Playing;
            self.mouse_dx = 0.0;
            self.mouse_dy = 0.0;
            // If the user picked a Load slot the Worlds picker reset
            // `last_built_worldgen` to None, which forces this branch
            // to load the slot's saved chunks (via the legacy
            // `load_server_state` shim that now dispatches into
            // world_save). If nothing changed since the last Play,
            // skip the heavy work so a quick menu round-trip doesn't
            // reset the world.
            let want_wg = WorldGen::current();
            if self.last_built_worldgen != Some(want_wg) {
                self.reset_world();
                self.build_world();
                let spawn = self.safe_spawn();
                self.player_pos = spawn;
                self.player_vel = Vec3::ZERO;
                self.fall_start_y = None;
                // Now that the procedural baseline is in place,
                // overlay the slot's persisted chunks + player. A
                // missing slot is fine - load_server_state swallows
                // NotFound and we keep the fresh world.
                self.load_server_state();
            }
        }
        // Recipe-book auto-fill: try to pull every input stack
        // from `self.inventory` into the 2x2 craft grid. Returns
        // any earlier withdrawals if the recipe demands more
        // than the player carries.
        // Clients ship the recipe index to the server which
        // mutates the authoritative inventory + replies with a
        // fresh `vx.inv` snapshot. Offline / host runs the
        // helper locally below.
        if let Some(idx) = prefill_recipe_idx
            && let NetRole::Client { client, .. } = &self.net
            && let Ok(buf) = bincode::serde::encode_to_vec(
                idx as u32,
                bincode::config::standard(),
            )
        {
            let _ = client.send_bytes("vx.recipe", buf);
        }
        if let Some(idx) = prefill_recipe_idx
            && !matches!(self.net, NetRole::Client { .. })
            && let Some(recipe) = RECIPES.get(idx)
        {
            // Inventory of (slot_idx, original_stack) we may
            // have to refund on failure.
            let mut taken: Vec<(usize, (BlockId, u8))> = Vec::new();
            let mut placed: [Option<(BlockId, u8)>; 4] = [None; 4];
            let mut ok = true;
            for (i, (bid, n)) in recipe.inputs.iter().enumerate() {
                if i >= 4 {
                    break;
                }
                let need = *n;
                let target = BlockId(*bid);
                // Look for a slot that has the needed block.
                let mut found_slot: Option<usize> = None;
                for (si, slot) in self.inventory.iter().enumerate() {
                    if let Some((b, c)) = slot
                        && *b == target
                        && *c >= need
                    {
                        found_slot = Some(si);
                        break;
                    }
                }
                match found_slot {
                    Some(si) => {
                        let original =
                            self.inventory[si].expect("verified above");
                        taken.push((si, original));
                        let remaining =
                            original.1.saturating_sub(need);
                        self.inventory[si] = if remaining == 0 {
                            None
                        } else {
                            Some((original.0, remaining))
                        };
                        placed[i] = Some((target, need));
                    }
                    None => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                // Return whatever was in the craft grid before
                // the auto-fill so it's not silently lost.
                let returns: Vec<(BlockId, u8)> = self
                    .craft_grid
                    .iter_mut()
                    .filter_map(|s| s.take())
                    .collect();
                for (b, c) in returns {
                    for _ in 0..c {
                        self.pickup(b);
                    }
                }
                self.craft_grid = placed;
            } else {
                // Roll back.
                for (si, original) in taken {
                    self.inventory[si] = Some(original);
                }
                self.chat_log.push((
                    std::time::Instant::now(),
                    format!(
                        "Not enough materials for {}",
                        recipe.name
                    ),
                ));
            }
        }
        if action_resume {
            self.state = AppState::Playing;
            self.mouse_dx = 0.0;
            self.mouse_dy = 0.0;
        }
        if action_quit {
            self.requested_quit = true;
        }
        if action_mp_resume {
            self.mp_menu_open = false;
        }
        if action_mp_disconnect {
            // Best-effort vx.leave so the server pre-empts its
            // 5s silence reaper + broadcasts the removal right
            // away. Then drop the net role + return to the main
            // menu so the user can rejoin / host again.
            self.notify_server_leaving();
            self.net = NetRole::Offline;
            self.mp_menu_open = false;
            self.state = AppState::MainMenu;
            self.remote_mobs.clear();
            self.mobs.clear();
        }
        if action_mp_quit {
            self.notify_server_leaving();
            self.requested_quit = true;
        }
        if action_menu_to_settings {
            self.state = AppState::Settings {
                back_to: BackTo::MainMenu,
            };
        }
        if action_pause_to_settings {
            self.state = AppState::Settings {
                back_to: BackTo::Pause,
            };
        }
        if action_back_settings && let AppState::Settings { back_to } = self.state {
            // Persist whatever the user just edited so the next
            // launch picks it up. Cheap (a few dozen bytes), so
            // doing it on Back is fine.
            self.save_config();
            self.state = match back_to {
                BackTo::MainMenu => AppState::MainMenu,
                BackTo::Pause => AppState::Paused,
            };
        }
        if action_main_menu {
            self.state = AppState::MainMenu;
        }
        if action_to_direct {
            self.state = AppState::DirectConnect;
        }
        if action_to_browser {
            self.state = AppState::ServerBrowser;
            self.browser_last_refresh = None;
            // Auto-refresh on entering the browser so the user
            // immediately sees the current directory contents.
            self.refresh_browser();
        }
        if action_to_host {
            self.state = AppState::HostSetup;
        }
        if action_join_direct {
            let addr = self.direct_addr_buf.clone();
            self.connect_as_client(&addr);
        }
        if let Some(addr) = action_join_browser {
            self.connect_as_client(&addr);
        }
        if action_browser_refresh {
            self.refresh_browser();
        }
        if action_host_start {
            self.start_hosting();
        }
        // Worlds picker (Phase B.7a scaffolding). Each branch
        // rewrites `current_world_slug` so the next save / load
        // operation targets the chosen slot; the actual load happens
        // lazily on Play offline via `load_server_state` so the user
        // sees the new world the moment they click Play.
        if action_new_world {
            let typed = self.new_world_name_buf.trim().to_string();
            let slug = if typed.is_empty() {
                world_save::DEFAULT_SLUG.to_string()
            } else {
                world_save::layout::slug(&typed)
            };
            // Mint a fresh meta + world.json right away so the slot
            // shows up in the picker on the next frame, even if the
            // user closes the app before clicking Play.
            let display = if typed.is_empty() { slug.clone() } else { typed };
            let meta = world_save::meta::create_meta(&display, &slug);
            if let Err(err) = world_save::meta::write_meta(&slug, &meta) {
                warn!(error = %err, slug = slug, "world_save: write_meta on New failed");
            }
            let wj = world_save::world_json::create_world(
                world_save::worldgen_str(WorldGen::current()),
                world_seed(),
            );
            if let Err(err) = world_save::world_json::write_world(&slug, &wj) {
                warn!(error = %err, slug = slug, "world_save: write_world on New failed");
            }
            self.current_world_slug = Some(slug);
            self.new_world_name_buf.clear();
        }
        if let Some(slug) = action_load_world {
            self.current_world_slug = Some(slug);
            // Trigger a fresh load on the next Play click by resetting
            // the world. `load_server_state` then pulls the slot in.
            self.reset_world();
            self.last_built_worldgen = None;
        }
        if let Some(slug) = action_delete_world {
            let dir = world_save::layout::world_dir(&slug);
            match std::fs::remove_dir_all(&dir) {
                Ok(()) => {
                    info!(slug = %slug, "world_save: deleted slot");
                    if self.current_world_slug.as_deref() == Some(slug.as_str()) {
                        self.current_world_slug =
                            Some(world_save::DEFAULT_SLUG.to_string());
                    }
                }
                Err(err) => warn!(
                    error = %err,
                    slug = %slug,
                    "world_save: delete slot failed"
                ),
            }
        }
    }
}

impl AppHandler for Voxel {
    fn on_start(&mut self, window: &EngineWindow) {
        let (w, h) = window.size();
        self.viewport = (w, h);
        // Load persisted settings before any user interaction so
        // the main menu badge + Settings panel show the last
        // chosen identity instead of a fresh random Player####.
        self.load_config();
        self.session_started = Some(std::time::Instant::now());
        // Drop the MOTD lines into the local chat log on
        // startup so the player sees the host's welcome banner
        // immediately. Empty/missing file = no-op.
        let motd = Self::motd_lines();
        if !motd.is_empty() {
            let now = std::time::Instant::now();
            for line in motd {
                self.chat_log.push((now, format!("[MOTD] {line}")));
            }
        }
        // Pick a default world seed for solo / offline play so
        // each cold start gets a different layout. Hosts override
        // via start_hosting; persisted state will eventually load
        // here once the save format carries a seed field.
        if world_seed() == 0 {
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
                .wrapping_mul(0x9E37_79B1)
                | 1;
            set_world_seed(seed);
        }
        info!(width = w, height = h, seed = world_seed(), "voxel_demo started");
        match Renderer::new(window) {
            Ok(r) => {
                let ui = EditorUi::new(r.wgpu_device(), r.surface_format(), window.raw(), 1);
                self.ui = Some(ui);
                self.renderer = Some(r);
            }
            Err(err) => {
                error!(error = %err, "renderer init failed");
                std::process::exit(2);
            }
        }
        // Build the procedural block atlas once + cache the shared
        // bind group. Every chunk submesh clones the Arc when it
        // creates its mesh handle so the GPU only pays the upload
        // cost once at startup.
        if let Some(r) = self.renderer.as_mut() {
            let (aw, ah, atlas_rgba) = self
                .texture_dir
                .as_ref()
                .and_then(|d| load_block_atlas_from_dir(d))
                .unwrap_or_else(build_block_atlas);
            self.voxel_atlas = Some(r.make_mesh_albedo(aw, ah, &atlas_rgba));
            // Keep the pixel data around for the egui inventory UI.
            self.atlas_rgba = Some((aw, ah, atlas_rgba));
            // Sheep body texture: a plain wool tile. We deliberately
            // don't use the Minecraft entity `sheep.tga` here -
            // that's a UV-mapped atlas for a multi-cube model
            // (head/body/legs split across face strips), and
            // wrapping it onto our single cube produces stripey
            // holes. The wool block tile, on the other hand, has
            // no UV layout assumptions: every face looks like wool.
            //
            // Alpha is force-cleared to 255 so the global cutout
            // shader path (`alpha < 0.5 -> discard`) can't poke
            // gaps through soft edges in the source PNG.
            if let Some(dir) = self.texture_dir.as_ref() {
                let path = dir.join("wool.png");
                if path.exists()
                    && let Ok(mut img) = image::open(&path).map(|i| i.to_rgba8())
                {
                    for px in img.chunks_exact_mut(4) {
                        px[3] = 255;
                    }
                    let mesh = r.create_cube_mesh("voxel_demo_mp::sheep_body");
                    let albedo =
                        r.make_mesh_albedo(img.width(), img.height(), &img);
                    r.bind_mesh_albedo_arc(mesh, albedo);
                    info!(path = %path.display(), "sheep body texture bound");
                    self.sheep_mesh = Some(mesh);
                }
                // Sheep head: the source `sheep.tga` is a UV atlas
                // for the full MC sheep model. We extract just the
                // face region + tile it across all 6 faces of a
                // dedicated head cube. Not pixel-perfect (the
                // proper fix is per-face UVs on a custom mesh) but
                // every visible side of the head shows the face,
                // which reads as a sheep from any angle.
                //
                // MC sheep texture layout (relative to the head
                // strip): the front face sits at U=[8/64, 16/64],
                // V=[8/32, 16/32] - i.e. the second 8x8 tile from
                // the left, second row. Bedrock's `sheep.tga` is
                // 64x64 (Java is 64x32), so we scale by texture
                // height to land on the same logical pixels.
                let sheep_root = dir
                    .parent()
                    .unwrap_or(dir.as_path())
                    .join("entity")
                    .join("sheep");
                let sheep_paths = [
                    sheep_root.join("sheep.tga"),
                    sheep_root.join("sheep.png"),
                    dir.join("sheep.tga"),
                    dir.join("sheep.png"),
                ];
                let sheep_src = sheep_paths.iter().find(|p| p.exists()).cloned();
                if let Some(path) = sheep_src
                    && let Ok(mut img) = image::open(&path).map(|i| i.to_rgba8())
                {
                    let w = img.width();
                    let h = img.height();
                    // Force alpha=255 so the cutout shader can't
                    // poke holes through soft edges in the source.
                    for px in img.chunks_exact_mut(4) {
                        px[3] = 255;
                    }
                    // Build a head cube with per-face UVs into the
                    // standard sheep texture layout (8x8 face tiles
                    // at the head strip on a 64-wide texture). Each
                    // face only shows the right region of the
                    // texture: front gets the face, sides get the
                    // wool sides, top gets the head crown.
                    let (verts, indices) = build_sheep_head_mesh(w, h);
                    let mesh = r.create_mesh("voxel_demo_mp::sheep_head", &verts, &indices);
                    let albedo = r.make_mesh_albedo(w, h, &img);
                    r.bind_mesh_albedo_arc(mesh, albedo);
                    info!(
                        path = %path.display(),
                        texture = format!("{w}x{h}"),
                        "sheep head per-face UV mesh bound"
                    );
                    self.sheep_head_mesh = Some(mesh);
                }
            }
        }
        self.workers = Some(WorkerPool::spawn(NUM_WORKERS));
        self.build_world();
        // Place the player on top of the actual terrain so the
        // hardcoded (0, 32, 0) origin doesn't drop them inside a
        // stone column when the seed happens to put a mountain
        // at the world origin.
        let spawn = self.safe_spawn();
        self.player_pos = spawn;
        self.player_vel = Vec3::ZERO;
        self.fall_start_y = None;
        // Best-effort audio: a missing output device just disables
        // SFX rather than blocking the demo from launching.
        match infengine_audio::AudioSystem::new() {
            Ok(a) => self.audio = Some(a),
            Err(err) => {
                warn!(error = %err, "audio init failed - SFX disabled");
            }
        }
        // Load user-supplied sound assets from `--sounds <dir>`.
        // Routed through the engine-level `AudioBank` helper so
        // shuffling + distance roll-off is reusable across demos.
        if let (Some(a), Some(dir)) = (self.audio.as_ref(), self.sounds_dir.as_ref()) {
            let scans: &[(&str, &str, &[&str])] = &[
                ("dig:grass", "dig", &["grass1.ogg", "grass2.ogg", "grass3.ogg", "grass4.ogg"]),
                ("dig:stone", "dig", &["stone1.ogg", "stone2.ogg", "stone3.ogg", "stone4.ogg"]),
                ("dig:sand", "dig", &["sand1.ogg", "sand2.ogg", "sand3.ogg", "sand4.ogg"]),
                ("dig:wood", "dig", &["wood1.ogg", "wood2.ogg", "wood3.ogg", "wood4.ogg"]),
                ("dig:gravel", "dig", &["gravel1.ogg", "gravel2.ogg", "gravel3.ogg", "gravel4.ogg"]),
                ("dig:glass", "dig", &["glass1.ogg", "glass2.ogg", "glass3.ogg"]),
                ("step:grass", "step", &["grass1.ogg", "grass2.ogg", "grass3.ogg", "grass4.ogg"]),
                ("step:stone", "step", &["stone1.ogg", "stone2.ogg", "stone3.ogg", "stone4.ogg"]),
                ("step:sand", "step", &["sand1.ogg", "sand2.ogg", "sand3.ogg", "sand4.ogg"]),
                ("step:wood", "step", &["wood1.ogg", "wood2.ogg", "wood3.ogg", "wood4.ogg"]),
                ("step:gravel", "step", &["gravel1.ogg", "gravel2.ogg", "gravel3.ogg", "gravel4.ogg"]),
                (
                    "mob/sheep/say",
                    "mob/sheep",
                    &["say1.ogg", "say2.ogg", "say3.ogg"],
                ),
                (
                    "mob/sheep/step",
                    "mob/sheep",
                    &["step1.ogg", "step2.ogg", "step3.ogg", "step4.ogg", "step5.ogg"],
                ),
                // Water splash variants. Different packs file
                // these under different subdirs; the scanner tries
                // each and keeps whatever loads. Synth fallback
                // (`play_thud` + `play_swish`) handles the empty
                // pack case.
                (
                    "splash",
                    "random",
                    &["splash1.ogg", "splash2.ogg", "splash3.ogg", "splash.ogg"],
                ),
                (
                    "splash",
                    "liquid",
                    &["splash.ogg", "splash1.ogg", "splash2.ogg"],
                ),
                (
                    "splash",
                    "water",
                    &["splash.ogg", "splash1.ogg", "splash2.ogg"],
                ),
                (
                    "splash",
                    "splash",
                    &["splash1.ogg", "splash2.ogg", "splash3.ogg", "splash.ogg"],
                ),
                // Liquid exit ambience. Used as the "splash_out"
                // cue; same pool can drive a future swim-loop
                // ambient.
                (
                    "splash_out",
                    "liquid",
                    &["water.ogg", "lava.ogg"],
                ),
                (
                    "splash_out",
                    "random",
                    &["splash.ogg"],
                ),
                // Running-water ambient loop. Used by the
                // periodic `water_flow` sting while a player
                // stands in a flow voxel.
                (
                    "water_flow",
                    "liquid",
                    &["water.ogg", "stream.ogg", "flow.ogg"],
                ),
                (
                    "water_flow",
                    "ambient",
                    &["water.ogg", "stream.ogg"],
                ),
            ];
            for (key, subdir, files) in scans {
                let paths: Vec<std::path::PathBuf> = files
                    .iter()
                    .map(|f| dir.join(subdir).join(f))
                    .filter(|p| p.exists())
                    .collect();
                if paths.is_empty() {
                    continue;
                }
                let n = self.audio_bank.load_variants(a, key, &paths);
                if n > 0 {
                    info!(key = %key, count = n, "loaded sound asset variants");
                }
            }
        }
        // Server hosts AND offline singleplayer resume any
        // persisted edits so a restart picks up where the world
        // left off. Clients sync via the catch-up broadcast on
        // first heartbeat.
        if matches!(self.net, NetRole::Server { .. } | NetRole::Offline) {
            self.load_server_state();
        }
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
        let dt = ctx.delta_time.clamp(1e-4, 0.1);
        // FPS counter: heavier new-frame weight so the reading
        // tracks reality under load. The old 0.9/0.1 EMA was so
        // sticky that a freshly-stalled game still showed the
        // previous steady-state value for seconds. Clamp the
        // displayed value to a sensible range too.
        let inst = (1.0 / dt.max(1e-3)).clamp(1.0, 999.0);
        self.fps_ema = self.fps_ema * 0.5 + inst * 0.5;

        // Pull any pending master-query results back onto the main
        // thread (refresh + announce both run on background workers).
        self.poll_master_queries();

        // Esc behaviour depends on net role:
        //  - Offline: classic Play <-> Paused toggle (whole world
        //    halts while the pause overlay is up).
        //  - Server / Client: world must keep running for the
        //    other peers, so Esc just toggles `mp_menu_open` which
        //    drives an overlay UI; physics / net tick continue.
        // Settings always backs out via Esc regardless of role.
        let esc_now = ctx.input.is_pressed(Key::Escape);
        let esc_just = esc_now && !self.esc_prev;
        self.esc_prev = esc_now;
        if esc_just {
            let mp = !matches!(self.net, NetRole::Offline);
            match self.state {
                AppState::Playing => {
                    // Chat input takes priority - Esc just dismisses
                    // it without opening the MP menu underneath.
                    // The chest UI takes second priority - Esc
                    // closes it before falling through to the
                    // mp-menu / pause path.
                    if self.chat_open {
                        self.chat_open = false;
                        self.chat_input.clear();
                    } else if self.chest_open.is_some() {
                        self.close_chest_ui();
                    } else if self.furnace_open.is_some() {
                        self.close_furnace_ui();
                    } else if mp {
                        self.mp_menu_open = !self.mp_menu_open;
                    } else {
                        self.state = AppState::Paused;
                    }
                }
                AppState::Paused => self.state = AppState::Playing,
                AppState::Settings { back_to } => {
                    self.state = match back_to {
                        BackTo::MainMenu => AppState::MainMenu,
                        BackTo::Pause => AppState::Paused,
                    };
                }
                AppState::MainMenu
                | AppState::DirectConnect
                | AppState::ServerBrowser
                | AppState::HostSetup => {}
            }
        }

        // E toggles the inventory overlay while playing. When the
        // overlay is open we release the cursor + freeze player
        // input so the UI is interactable.
        // F3 toggles the debug panel. Mirrors Minecraft's debug
        // overlay: cheap to leave running, off by default.
        let f3_now = ctx.input.is_pressed(Key::F3);
        if f3_now
            && !self.f3_prev
            && matches!(self.state, AppState::Playing)
            && !self.chat_open
        {
            self.show_debug = !self.show_debug;
        }
        self.f3_prev = f3_now;
        // F1 hides / shows the entire HUD (hotbar, crosshair,
        // health, mob bars). Useful for screenshots + benchmarks.
        let f1_now = ctx.input.is_pressed(Key::F1);
        if f1_now
            && !self.f1_prev
            && matches!(self.state, AppState::Playing)
            && !self.chat_open
        {
            self.hud_visible = !self.hud_visible;
        }
        self.f1_prev = f1_now;
        // F5 cycles the camera through first-person and two
        // third-person poses (behind, then in front). Disabled
        // while chat is open so accidental F5 mid-typing doesn't
        // disorient the player.
        let f5_now = ctx.input.is_pressed(Key::F5);
        if f5_now
            && !self.f5_prev
            && matches!(self.state, AppState::Playing)
            && !self.chat_open
        {
            self.camera_mode = (self.camera_mode + 1) % 3;
        }
        self.f5_prev = f5_now;
        // F6 toggles the live debug panel.
        let f6_now = ctx.input.is_pressed(Key::F6);
        if f6_now
            && !self.f6_prev
            && matches!(self.state, AppState::Playing)
            && !self.chat_open
        {
            self.show_debug_panel = !self.show_debug_panel;
        }
        self.f6_prev = f6_now;
        // F4 cycles weather: clear -> rain -> snow -> clear.
        // Broadcasts when hosting so peers follow the change.
        let f4_now = ctx.input.is_pressed(Key::F4);
        if f4_now
            && !self.f4_prev
            && matches!(self.state, AppState::Playing)
            && !self.chat_open
        {
            self.weather = (self.weather + 1) % 3;
            if let NetRole::Server { server, .. } = &mut self.net
                && let Ok(buf) = bincode::serde::encode_to_vec(
                    self.weather,
                    bincode::config::standard(),
                )
            {
                server.broadcast_bytes("vx.weather", buf);
            }
            let label = match self.weather {
                1 => "Rain",
                2 => "Snow",
                _ => "Clear",
            };
            self.chat_log.push((
                std::time::Instant::now(),
                format!("Weather: {label}"),
            ));
        }
        self.f4_prev = f4_now;
        // Q drops one item from the selected hotbar slot. No
        // dropped-entity model yet so the item simply
        // disappears - useful for trimming overstocked stacks.
        let q_now = ctx.input.is_pressed(Key::Q);
        if q_now
            && !self.q_prev
            && matches!(self.state, AppState::Playing)
            && !self.chat_open
            && !self.inventory_open
            && self.selected_slot < HOTBAR_SLOTS
            && let Some((b, c)) = self.inventory[self.selected_slot]
        {
            // Ctrl held = drop the entire stack at once.
            let drop_all = ctx.input.is_pressed(Key::ControlLeft)
                || ctx.input.is_pressed(Key::ControlRight);
            let new_count = if drop_all { 0 } else { c.saturating_sub(1) };
            self.inventory[self.selected_slot] = if new_count == 0 {
                None
            } else {
                Some((b, new_count))
            };
            // Small puff at the player's feet so the drop is
            // visible; mirrors the break-particle texture so
            // identifying the block stays easy.
            let color = self
                .registry
                .get(b)
                .map(|d| d.color)
                .unwrap_or([0.8, 0.8, 0.8]);
            let voxel = [
                self.player_pos.x.floor() as i32,
                (self.player_pos.y - PLAYER_HEIGHT - 0.05).floor() as i32,
                self.player_pos.z.floor() as i32,
            ];
            self.spawn_break_particles_tinted(voxel, color);
        }
        self.q_prev = q_now;
        // Z undoes the last placement. Routes through the edit
        // pipeline so the air swap replicates + persists. Gated
        // on chat / inventory so accidental Z while typing
        // doesn't trash builds.
        let z_now = ctx.input.is_pressed(Key::Z);
        if z_now
            && !self.z_prev
            && matches!(self.state, AppState::Playing)
            && !self.chat_open
            && !self.inventory_open
            && let Some(voxel) = self.place_history.pop()
        {
            let current = self.world.get(voxel);
            if current != BlockId::AIR {
                let edit = EditMsg {
                    x: voxel[0],
                    y: voxel[1],
                    z: voxel[2],
                    block: 0,
                };
                self.world_set_tracked(voxel, BlockId::AIR);
                let coords = Self::affected_chunks(voxel);
                self.rebuild_chunks(&coords);
                let cfg = bincode::config::standard();
                match &mut self.net {
                    NetRole::Offline => {}
                    NetRole::Server { server, .. } => {
                        self.edit_history.push(edit);
                        if let Ok(buf) =
                            bincode::serde::encode_to_vec(edit, cfg)
                        {
                            server.broadcast_bytes("vx.edit", buf);
                        }
                    }
                    NetRole::Client { pending_edits, .. } => {
                        pending_edits.push(edit);
                    }
                }
                // Refund: pickup() handles stack merging so the
                // undone block returns to the inventory.
                self.pickup(current);
            }
        }
        self.z_prev = z_now;
        // G toggles the chunk-grid overlay. Cheap debug aid;
        // only fires when F3 debug is also showing to avoid
        // cluttering normal play.
        let g_now = ctx.input.is_pressed(Key::G);
        if g_now
            && !self.g_prev
            && matches!(self.state, AppState::Playing)
            && !self.chat_open
        {
            self.show_chunk_grid = !self.show_chunk_grid;
        }
        self.g_prev = g_now;
        // H cycles the targeting wireframe between Auto, Build and
        // Break. Drives the per-frame highlight render decision and
        // is persisted per-player in PlayerSave so the choice
        // survives /save + relaunch.
        let h_now = ctx.input.is_pressed(Key::H);
        if h_now
            && !self.h_prev
            && matches!(self.state, AppState::Playing)
            && !self.chat_open
        {
            self.highlight_mode = self.highlight_mode.cycle();
            let label = self.highlight_mode.label();
            self.chat_log.push((
                std::time::Instant::now(),
                format!("Highlight mode: {label}"),
            ));
        }
        self.h_prev = h_now;
        // Tab held = playerlist overlay. Hidden in singleplayer
        // (the overlay drawer also guards on net role, but the
        // flag is gated here too so debug panels stay clean).
        self.tab_held =
            matches!(self.state, AppState::Playing) && ctx.input.is_pressed(Key::Tab);
        // 1..9 select the matching hotbar slot directly (Minecraft
        // convention). Digit 1 maps to slot 0 so the user-facing
        // numbering matches the visual order in the hotbar.
        if matches!(self.state, AppState::Playing) && !self.chat_open {
            let digits = [
                Key::Digit1,
                Key::Digit2,
                Key::Digit3,
                Key::Digit4,
                Key::Digit5,
                Key::Digit6,
                Key::Digit7,
                Key::Digit8,
                Key::Digit9,
            ];
            for (i, key) in digits.iter().enumerate() {
                if ctx.input.is_pressed(*key) && i < HOTBAR_SLOTS {
                    self.selected_slot = i;
                    break;
                }
            }
        }

        let e_now = ctx.input.is_pressed(Key::E);
        let e_just = e_now && !self.e_prev;
        self.e_prev = e_now;
        if e_just
            && matches!(self.state, AppState::Playing)
            && !self.chat_open
        {
            // Pressing E while a chest / furnace UI is open closes
            // it first instead of stacking a second panel on top.
            if self.chest_open.is_some() {
                self.close_chest_ui();
            } else if self.furnace_open.is_some() {
                self.close_furnace_ui();
            } else {
                self.inventory_open = !self.inventory_open;
            }
            // Drop any queued mouse-button edits the click that
            // closes the inventory would otherwise be replayed
            // against the world.
            self.lmb_pending = false;
            self.rmb_pending = false;
        }

        // T opens the chat input. Only fires in MP roles + while
        // no other overlay is up. Esc + Enter both close it.
        let t_now = ctx.input.is_pressed(Key::T);
        let t_just = t_now && !self.t_prev;
        self.t_prev = t_now;
        if t_just
            && matches!(self.state, AppState::Playing)
            && !self.inventory_open
            && !self.mp_menu_open
            && !self.chat_open
        {
            // Chat opens in any net role (including Offline) so
            // local slash commands like /give work in
            // singleplayer too.
            self.chat_open = true;
            self.chat_input.clear();
        }

        // F = eat. Consumes one meat block (id 12) from anywhere in
        // the inventory + restores 4 HP. No-op when at full health
        // or when the player has no meat. Gated on Playing state +
        // no overlay so the F key doesn't fire while the menu eats
        // text input.
        let f_now = ctx.input.is_pressed(Key::F);
        let f_just = f_now && !self.f_prev;
        self.f_prev = f_now;
        if f_just
            && matches!(self.state, AppState::Playing)
            && !self.inventory_open
            && !self.mp_menu_open
            && !self.chat_open
            && self.player_health < PLAYER_MAX_HEALTH
        {
            // Find first slot holding meat (offline / host path).
            let mut ate = false;
            for slot in self.inventory.iter_mut() {
                if let Some((b, c)) = slot.as_mut()
                    && *b == BlockId(12)
                {
                    *c = c.saturating_sub(1);
                    if *c == 0 {
                        *slot = None;
                    }
                    ate = true;
                    break;
                }
            }
            if ate {
                self.player_health =
                    (self.player_health + 4.0).min(PLAYER_MAX_HEALTH);
                let pos = [self.player_pos.x, self.player_pos.y, self.player_pos.z];
                self.sfx_at("eat", pos, |a, g| {
                    let _ = a.play_thud(280.0, 0.18 * g);
                });
            }
        }

        if matches!(self.state, AppState::Playing) {
            // update_play itself gates player input on
            // `self.inventory_open` - we still need to tick it every
            // frame so gravity + collision + net pose broadcasts
            // keep running while the menu is up.
            self.update_play(ctx, dt);
            self.stream_chunks();
        } else {
            // Paused / in a menu screen: drop captured mouse motion
            // so the camera doesn't lurch when play resumes.
            self.mouse_dx = 0.0;
            self.mouse_dy = 0.0;
            self.wheel_accum = 0.0;
        }
        self.apply_cursor_settings(window);

        // Build UI state first so the immediate-mode pass mutates self
        // freely. The renderer is borrowed afterwards once UI state has
        // settled.
        self.render_ui(window);

        let forward = Vec3::new(
            self.player_yaw.cos() * self.player_pitch.cos(),
            self.player_pitch.sin(),
            -self.player_yaw.sin() * self.player_pitch.cos(),
        );

        // Cast a ray from the eye along the look direction so the
        // crosshair / wireframe / edit code can target the voxel the
        // player is currently aiming at. Pass-through decorations
        // (torch, flowers) are skipped so the ray reaches whatever
        // solid block lives behind them - placing a block adjacent
        // to a torch no longer fails when the crosshair clips the
        // torch's full-voxel hitbox.
        self.target = if matches!(self.state, AppState::Playing) {
            raycast_with(
                &self.world,
                self.player_pos,
                forward,
                6.0,
                |b| !b.is_air() && !block_is_ray_passable(b),
            )
        } else {
            None
        };
        // Separate ray for mining: stops on the FIRST non-air,
        // non-liquid voxel - decorations included. The
        // placement-target ray tunnels through `F_RAY_PASS`
        // blocks so the player can drop blocks next to a torch
        // without selecting it; this one stops at torches so
        // they can be mined. Liquids stay invisible to both
        // rays - they're not directly breakable (the bucket
        // scoop handles water/lava source pickup), and
        // wireframe-highlighting a falling stream would just
        // confuse the player.
        self.mine_target = if matches!(self.state, AppState::Playing) {
            raycast_with(
                &self.world,
                self.player_pos,
                forward,
                6.0,
                |b| !b.is_air() && !block_is_liquid(b),
            )
        } else {
            None
        };

        // Apply pending mouse-button edits before rendering. LMB
        // always breaks the targeted voxel. RMB places the selected
        // hotbar block at the adjacent empty cell, but is ignored
        // when the selected slot is empty or holds a tool/weapon.
        let lmb = self.lmb_pending;
        let rmb = self.rmb_pending;
        self.lmb_pending = false;
        self.rmb_pending = false;
        // The currently-held block: contents of the selected hotbar
        // slot. `None` = empty hand (or 0-count remnant).
        let selected_block = self.inventory[self.selected_slot].map(|(b, _)| b);
        // Net mode flag: clients defer block edits to the server +
        // wait for the server echo before applying. Offline + server
        // apply locally as before.
        let is_client = matches!(self.net, NetRole::Client { .. });
        // Mob attack interception: if LMB and a mob is closer than
        // the targeted voxel (or there's no voxel target), route
        // the click as a melee strike instead of a block break.
        let combat_gate = lmb && !self.any_panel_open();
        let mob_target = if combat_gate {
            self.raycast_mob(self.player_pos, forward, MOB_ATTACK_REACH)
        } else {
            None
        };
        let pvp_target = if combat_gate {
            self.raycast_remote_player(self.player_pos, forward, PVP_REACH)
        } else {
            None
        };
        let voxel_dist = self.target.map(|h| {
            let c = Vec3::new(
                h.voxel[0] as f32 + 0.5,
                h.voxel[1] as f32 + 0.5,
                h.voxel[2] as f32 + 0.5,
            );
            (c - self.player_pos).length()
        });
        // Pick whichever attack target is closest: player < mob <
        // voxel. Voxel break path is suppressed when any combat
        // target wins.
        let mut best_dist = f32::INFINITY;
        let mut attack_player: Option<u32> = None;
        let mut attack_mob: Option<u32> = None;
        if let Some((id, d)) = pvp_target
            && d < best_dist
        {
            best_dist = d;
            attack_player = Some(id);
            attack_mob = None;
        }
        if let Some((id, d)) = mob_target
            && d < best_dist
        {
            best_dist = d;
            attack_mob = Some(id);
            attack_player = None;
        }
        if let Some(d) = voxel_dist
            && d < best_dist
        {
            attack_mob = None;
            attack_player = None;
        }
        // Combat consumes a flat 3 stamina per attack so the
        // sprint pool is shared between movement and fighting.
        if attack_player.is_some() || attack_mob.is_some() {
            self.stamina = (self.stamina - 3.0).max(0.0);
        }
        // Player-vs-player attack: dispatch to server-side damage
        // application. Host applies directly; client sends a
        // `vx.pvp_hit` request the server can validate.
        if let Some(target_id) = attack_player {
            let held = self
                .inventory
                .get(self.selected_slot)
                .copied()
                .unwrap_or(None)
                .map(|(b, _)| b)
                .unwrap_or(BlockId::AIR);
            let mult = weapon_damage_mult(held);
            match &self.net {
                NetRole::Client { client, .. } => {
                    let cfg = bincode::config::standard();
                    let msg = PvPHitMsg { target_id };
                    if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                        let _ = client.send_bytes("vx.pvp_hit", buf);
                    }
                }
                NetRole::Server { server, .. } => {
                    // Host is attacker id 0 -> send DamageMsg
                    // straight to the target.
                    let cfg = bincode::config::standard();
                    let dmg = DamageMsg {
                        amount: PVP_DAMAGE * mult,
                    };
                    if let Ok(buf) = bincode::serde::encode_to_vec(dmg, cfg) {
                        server.send_bytes_to_client(target_id, "vx.damage", buf);
                    }
                }
                NetRole::Offline => {}
            }
        }
        if let Some(mob_id) = attack_mob {
            // Consume the LMB so it doesn't also break a voxel.
            if is_client {
                // Client: send hit request to server, optimistic
                // flash so the player gets immediate feedback.
                let msg = MobHitMsg { mob_id };
                if let NetRole::Client { client, .. } = &self.net {
                    let cfg = bincode::config::standard();
                    if let Ok(buf) = bincode::serde::encode_to_vec(msg, cfg) {
                        let _ = client.send_bytes("vx.mob_hit", buf);
                    }
                }
                self.mob_hurt_flash.insert(mob_id, 0.30);
                if let Some(mob) = self.remote_mobs.get(&mob_id).copied() {
                    let pos = mob.pos;
                    self.sfx_at("mob/sheep/say", pos, |a, g| {
                        let _ = a.play_beep(280.0, 0.12, 0.18 * g);
                    });
                }
            } else {
                // Offline / host: authoritative damage path. Host
                // is attacker id 0. Weapon tier scales the hit.
                let held = self
                    .inventory
                    .get(self.selected_slot)
                    .copied()
                    .unwrap_or(None)
                    .map(|(b, _)| b)
                    .unwrap_or(BlockId::AIR);
                let mult = weapon_damage_mult(held);
                self.damage_mob_scaled(mob_id, Some(0), mult);
            }
        }
        // Mining: LMB held + a voxel in view, but no mob in front
        // (a melee swing takes priority). Tick the active session
        // toward `total`; on completion, set `mine_break` so the
        // block-edit path below executes the break exactly once.
        let lmb_active = self.lmb_held
            && !self.any_panel_open()
            && attack_mob.is_none()
            && attack_player.is_none();
        let mut mine_break: Option<[i32; 3]> = None;
        if lmb_active && let Some(hit) = self.mine_target {
            let block = self.world.get(hit.voxel);
            // Standard ray skips liquid voxels (F_RAY_PASS) so
            // the crosshair targets the solid block past any water
            // or lava in the way - players can dig through a pond
            // to the seabed, the submerged_mining_mul slows the
            // swing. Liquids themselves remain unbreakable; they
            // only come out via the bucket scoop path.
            if block != BlockId::AIR {
                // Restart the session if either the target voxel
                // changed or the block at the target has been
                // replaced (e.g. by another player).
                let restart = match self.mining {
                    Some(s) => s.voxel != hit.voxel || s.block != block,
                    None => true,
                };
                if restart {
                    // Tool-class effectiveness drives the
                    // mining time. Shovels chew dirt fast, axes
                    // chew wood fast, pickaxes chew stone fast;
                    // the wrong tool is a 4x penalty.
                    let held = self
                        .inventory
                        .get(self.selected_slot)
                        .copied()
                        .unwrap_or(None)
                        .map(|(b, _)| b)
                        .unwrap_or(BlockId::AIR);
                    let (mut mult, _drop_ok) =
                        tool_effectiveness(held, block);
                    // Submerged mining: liquid slows the swing
                    // ONLY when the head is fully underwater
                    // (same gate as the screen tint). Wading
                    // ankle-deep mines at full speed.
                    if let Some(l) =
                        submerged_liquid_eye(&self.world, self.player_pos)
                    {
                        mult *= l.submerged_mining_mul;
                    }
                    self.mining = Some(MiningState {
                        voxel: hit.voxel,
                        progress: 0.0,
                        total: (block_hardness(block) * mult).max(0.05),
                        block,
                        fx_cooldown: 0.0,
                    });
                }
                // Pull the state out by value so we can call
                // `sfx_at` (which needs `&mut self`) without the
                // borrow checker complaining about overlap with
                // `self.mining`. Write the updated state back at
                // the end of the tick.
                if let Some(mut m) = self.mining.take() {
                    m.progress += dt;
                    m.fx_cooldown -= dt;
                    let done = m.progress >= m.total;
                    let voxel_center = [
                        m.voxel[0] as f32 + 0.5,
                        m.voxel[1] as f32 + 0.5,
                        m.voxel[2] as f32 + 0.5,
                    ];
                    let fire_fx = m.fx_cooldown <= 0.0 || done;
                    if fire_fx {
                        m.fx_cooldown = 0.22;
                    }
                    let key = format!("dig:{}", block_material(block));
                    let freq = 220.0 + (block.0 as f32) * 18.0;
                    let total = m.total;
                    let progress = m.progress;
                    if fire_fx {
                        self.sfx_at(&key, voxel_center, move |a, g| {
                            let _ = a.play_thud(
                                freq,
                                0.10 * g * (0.5 + 0.5 * progress / total),
                            );
                        });
                        // Spit a tiny block-tinted puff at each
                        // dig tick so the mining feedback reads
                        // visually too, matching the MC "crack"
                        // particles. Frequency matches the SFX
                        // cooldown.
                        let color = self
                            .registry
                            .get(block)
                            .map(|d| d.color)
                            .unwrap_or([0.7, 0.7, 0.7]);
                        self.spawn_break_particles_tinted(m.voxel, color);
                    }
                    if done {
                        mine_break = Some(m.voxel);
                    } else {
                        self.mining = Some(m);
                    }
                }
            } else {
                self.mining = None;
            }
        } else {
            self.mining = None;
        }
        // Suppress the legacy single-click break: mining now drives
        // it via `mine_break`. Keep `lmb` consumed so a fresh
        // press doesn't bleed through somewhere else.
        let _ = lmb;
        // Block break: fires once when `mine_break` is set (i.e.
        // the hold-to-mine session completed). The path applies
        // the world edit, broadcasts, credits the pickup, and
        // plays the break SFX.
        if !self.inventory_open
            && !self.mp_menu_open
            && let Some(voxel) = mine_break
        {
            // Capture the block we're about to break so we can
            // pop it into the inventory (greedy collection - air
            // just no-ops).
            let broken = self.world.get(voxel);
            // Tint the debris with the broken block's albedo so a
            // stone break sprays grey chips, wood sprays brown,
            // glass sprays pale blue, etc.
            let break_color = self
                .registry
                .get(broken)
                .map(|d| d.color)
                .unwrap_or([0.75, 0.55, 0.30]);
            let edit = EditMsg {
                x: voxel[0],
                y: voxel[1],
                z: voxel[2],
                block: 0,
            };
            if is_client {
                self.world_set_tracked(voxel, BlockId::AIR);
                let coords = Self::affected_chunks(voxel);
                self.rebuild_chunks(&coords);
                self.spawn_break_particles_tinted(voxel, break_color);
                if let NetRole::Client { pending_edits, .. } = &mut self.net {
                    pending_edits.push(edit);
                }
            } else {
                self.world_set_tracked(voxel, BlockId::AIR);
                let coords = Self::affected_chunks(voxel);
                self.rebuild_chunks(&coords);
                self.spawn_break_particles_tinted(voxel, break_color);
                if let NetRole::Server { server, .. } = &self.net {
                    let cfg = bincode::config::standard();
                    if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                        server.broadcast_bytes("vx.edit", buf);
                    }
                }
                self.edit_history.push(edit);
            }
            if !is_client && broken != BlockId::AIR {
                // Tool gating: drop only when the currently
                // selected slot's tool tier meets the block's
                // requirement. Player can still swing at higher
                // tier blocks but gets nothing back, matching
                // Minecraft's "wrong tool" semantics. The block
                // spawns as a floating drop the player walks
                // through to collect, instead of jumping
                // straight into the inventory.
                let held = self
                    .inventory
                    .get(self.selected_slot)
                    .copied()
                    .unwrap_or(None)
                    .map(|(b, _)| b)
                    .unwrap_or(BlockId::AIR);
                let tier = tool_tier(held);
                let needed = block_required_tier(broken);
                if tier >= needed {
                    self.spawn_drop(broken, voxel);
                }
                // Chest break: drain the per-position container's
                // contents as item drops at the same voxel, then
                // remove the container entry. We do this even when
                // the chest itself can't be picked up (e.g. wrong
                // tier) so the player never loses stored items.
                // `spawn_drop` emits one item entity per call, so
                // a stack of N produces N drops at the same voxel
                // (each gets a randomised XZ kick from `drops::spawn`,
                // so the cluster reads as a small pile).
                if broken.0 == CHEST_ID
                    && let Some(chest_inv) = self.chests.remove(&voxel)
                {
                    for id in block::chest::drain_drops(&chest_inv) {
                        self.spawn_drop(BlockId(id), voxel);
                    }
                }
                // Furnace break: drain the 3 slots (input / fuel /
                // output) as drops, then drop the runtime entry too.
                // `break_furnace` collapses the two into one call so
                // a stray runtime entry can't linger after the slot
                // vec is gone. The Timer::Furnace entry on the
                // scheduled-timers list ages out naturally on the
                // next save (no live timer = no entry built).
                if broken.0 == FURNACE_ID {
                    let drops = block::furnace::break_furnace(
                        &mut self.furnaces,
                        &mut self.furnace_runtimes,
                        voxel,
                    );
                    for id in drops {
                        self.spawn_drop(BlockId(id), voxel);
                    }
                }
                // Cascade: dislodge any F_NO_PLACE decoration
                // (torch, flower, ...) that just lost its support
                // because we broke this voxel. Each dislodged
                // block also spawns as a drop, so the player can
                // pick it up. MC behaviour - mining a dirt block
                // under a torch makes the torch pop off + drop.
                self.dislodge_unsupported_decorations(voxel);
            }
            if broken != BlockId::AIR {
                self.blocks_broken = self.blocks_broken.saturating_add(1);
            }
            // Final break SFX (the loop already plays intermediate
            // dig sounds while the player holds LMB).
            let voxel_center = [
                voxel[0] as f32 + 0.5,
                voxel[1] as f32 + 0.5,
                voxel[2] as f32 + 0.5,
            ];
            let key = format!("dig:{}", block_material(broken));
            let freq = 220.0 + (broken.0 as f32) * 28.0;
            self.sfx_at(&key, voxel_center, move |a, g| {
                let _ = a.play_thud(freq, 0.30 * g);
            });
        }
        // Empty-bucket scoop: runs BEFORE the placement-target
        // chain because `self.target` is the placement ray which
        // tunnels through `F_RAY_PASS` liquids - if the player
        // points an empty bucket at a water source with sky
        // behind it, `self.target` is `None` and the bucket
        // branch inside the chain below would never fire. This
        // path runs its own purpose-built raycast that DOES
        // stop at liquid voxels.
        let bucket_scooped = if !self.any_panel_open()
            && attack_mob.is_none()
            && rmb
            && let Some(held) = selected_block
            && held.0 == BUCKET_EMPTY_ID
        {
            self.try_scoop_with_bucket(forward)
        } else {
            false
        };
        // Block place: RMB single click stays instant; no mining.
        if !bucket_scooped
            && !self.any_panel_open()
            && attack_mob.is_none()
            && let Some(hit) = self.target
        {
            if false {
                // Sentinel arm so the existing `else if rmb ...`
                // structure stays a single `if/else if` chain.
                unreachable!()
            } else if rmb
                && self.world.get(hit.voxel).0 == 3
                && {
                    let held = self
                        .inventory
                        .get(self.selected_slot)
                        .copied()
                        .unwrap_or(None)
                        .map(|(b, _)| b)
                        .unwrap_or(BlockId::AIR);
                    shovel_tier(held) > 0
                }
            {
                // Shovel-RMB on grass -> degrade to dirt
                // (Minecraft's "path" shortcut). Apply locally
                // for instant visual feedback then route the
                // change through the right MP path: host
                // broadcasts vx.edit, client sends vx.path and
                // the server replays / rejects.
                let voxel = hit.voxel;
                let cfg = bincode::config::standard();
                self.world_set_tracked(voxel, BlockId(2));
                let coords = Self::affected_chunks(voxel);
                self.rebuild_chunks(&coords);
                let edit = EditMsg {
                    x: voxel[0],
                    y: voxel[1],
                    z: voxel[2],
                    block: 2,
                };
                match &mut self.net {
                    NetRole::Offline => {}
                    NetRole::Server { server, .. } => {
                        self.edit_history.push(edit);
                        if let Ok(buf) =
                            bincode::serde::encode_to_vec(edit, cfg)
                        {
                            server.broadcast_bytes("vx.edit", buf);
                        }
                    }
                    NetRole::Client { client, .. } => {
                        if let Ok(buf) =
                            bincode::serde::encode_to_vec(voxel, cfg)
                        {
                            let _ =
                                client.send_bytes("vx.path", buf);
                        }
                    }
                }
            } else if rmb && self.world.get(hit.voxel).0 == FURNACE_ID {
                // RMB on a furnace: open the furnace UI. Mints a
                // fresh empty 3-slot container + runtime entry on
                // first open so the offline place-without-open
                // case still gets a clean state to work against.
                let pos = hit.voxel;
                block::furnace::place_furnace(
                    &mut self.furnaces,
                    &mut self.furnace_runtimes,
                    pos,
                );
                self.furnace_open = Some(pos);
                self.lmb_pending = false;
                self.rmb_pending = false;
            } else if rmb && self.world.get(hit.voxel).0 == CHEST_ID {
                // RMB on a chest: open the chest UI for the chest
                // at this voxel. Host-local + offline paths set
                // the open-chest flag directly; the chest map is
                // already authoritative on this peer (clients
                // received the place via `vx.edit` but the C.3
                // MP wire for chest state is deferred to C.3f).
                //
                // TODO(C.3f): when running as `NetRole::Client`,
                // ship a `vx.chest_open` to the host and treat
                // subsequent `vx.chest_state` broadcasts as the
                // authoritative chest contents. Today every peer
                // mints its own local empty chest on open, so
                // chest contents only persist + sync on the
                // peer that placed + filled the chest. The save
                // round trip still works on the host because
                // `voxel.chests` lives on the host side.
                let pos = hit.voxel;
                self.chests
                    .entry(pos)
                    .or_insert_with(block::chest::new_chest);
                self.chest_open = Some(pos);
                // Drop any queued mouse-button edits the click
                // that opened the chest would otherwise replay
                // against the world.
                self.lmb_pending = false;
                self.rmb_pending = false;
            } else if rmb && self.world.get(hit.voxel).0 == 35 {
                // RMB on a bed: night-time, run the local /sleep
                // logic if we're authoritative; clients ship a
                // vx.sleep request to the host so the world
                // clock advances for everyone.
                let phase = (self.day_time / DAY_LENGTH_S).rem_euclid(1.0);
                let elev = (phase * std::f32::consts::TAU).sin();
                let is_night = elev < -0.05;
                if !is_night {
                    self.chat_log.push((
                        std::time::Instant::now(),
                        "You can only sleep at night.".to_string(),
                    ));
                } else {
                    match &self.net {
                        NetRole::Offline | NetRole::Server { .. } => {
                            self.day_time = DAY_LENGTH_S * 0.20;
                            self.last_day_broadcast = None;
                            self.chat_log.push((
                                std::time::Instant::now(),
                                "* sleeps through the night"
                                    .to_string(),
                            ));
                        }
                        NetRole::Client { client, .. } => {
                            let _ = client
                                .send_bytes("vx.sleep", Vec::new());
                            self.chat_log.push((
                                std::time::Instant::now(),
                                "Asking host to skip the night..."
                                    .to_string(),
                            ));
                        }
                    }
                }
            } else if rmb
                && let Some(held) = selected_block
                && held.0 == block::FLINT_AND_STEEL_ID
            {
                // Phase F.5: RMB with flint+steel on an obsidian
                // block ignites a valid portal frame. The hit
                // cell is the obsidian block itself; the frame
                // interior is one step away on the clicked face,
                // which is `hit.place_at`. Pass that to the
                // ignition routine.
                let cfg = bincode::config::standard();
                let res = block::portal::try_ignite(&mut self.world, hit.place_at);
                if let block::portal::IgniteResult::Ok { interior_count, .. } = res {
                    // Broadcast the interior changes as standard
                    // world edits so remote peers see the portal
                    // appear. The portal block id is set on
                    // every interior cell; emit one EditMsg per
                    // cell. The cells form a tight rect around
                    // hit.place_at so the radius isn't huge.
                    let center = hit.place_at;
                    // Re-scan the 3x3x3 around the clicked cell
                    // to pick up every portal cell we just wrote.
                    let mut emitted: Vec<EditMsg> = Vec::with_capacity(interior_count as usize);
                    for dx in -3..=3 {
                        for dy in -3..=3 {
                            for dz in -3..=3 {
                                let p = [center[0] + dx, center[1] + dy, center[2] + dz];
                                if self.world.get(p).0 == block::PORTAL_ID {
                                    emitted.push(EditMsg {
                                        x: p[0],
                                        y: p[1],
                                        z: p[2],
                                        block: block::PORTAL_ID,
                                    });
                                }
                            }
                        }
                    }
                    let coords = Self::affected_chunks(hit.place_at);
                    self.rebuild_chunks(&coords);
                    match &mut self.net {
                        NetRole::Offline => {}
                        NetRole::Server { server, .. } => {
                            for edit in &emitted {
                                self.edit_history.push(*edit);
                                if let Ok(buf) =
                                    bincode::serde::encode_to_vec(*edit, cfg)
                                {
                                    server.broadcast_bytes("vx.edit", buf);
                                }
                            }
                        }
                        NetRole::Client { pending_edits, .. } => {
                            for edit in &emitted {
                                pending_edits.push(*edit);
                            }
                        }
                    }
                    self.chat_log.push((
                        std::time::Instant::now(),
                        "Portal lit.".to_string(),
                    ));
                } else if matches!(
                    res,
                    block::portal::IgniteResult::NotRectangular
                        | block::portal::IgniteResult::PerimeterIncomplete
                ) {
                    self.chat_log.push((
                        std::time::Instant::now(),
                        "Portal frame is invalid.".to_string(),
                    ));
                }
                // Flint+steel doesn't deplete (durability is a
                // deferred follow-up - see Phase F module docs).
                self.lmb_pending = false;
                self.rmb_pending = false;
            } else if rmb
                && let Some(held) = selected_block
                && matches!(
                    held.0,
                    BUCKET_EMPTY_ID | BUCKET_WATER_ID | BUCKET_LAVA_ID
                )
            {
                // Bucket RMB interactions.
                //   empty bucket  + liquid voxel -> scoop, swap to
                //                                   filled bucket.
                //   filled bucket + air / replaceable cell -> emit
                //                                   source, swap to
                //                                   empty bucket.
                // The standard target ray skips liquids (they're
                // F_RAY_PASS) so we run a separate cast for the
                // scoop path that stops at the first liquid voxel
                // OR the first solid block, whichever comes first.
                let cfg = bincode::config::standard();
                if held.0 == BUCKET_EMPTY_ID {
                    let liquid_hit = raycast_with(
                        &self.world,
                        self.player_pos,
                        forward,
                        6.0,
                        |b| !b.is_air() && (block_is_liquid(b) || !block_is_ray_passable(b)),
                    );
                    let scoop_voxel = liquid_hit.map(|h| h.voxel);
                    let scoop_block = scoop_voxel
                        .map(|v| self.world.get(v))
                        .unwrap_or(BlockId::AIR);
                    // Scoop only from a source voxel (level 0).
                    // Flow / falling cells can't be bottled in MC -
                    // the bucket pops back empty when aimed at a
                    // stream.
                    let scoop_level = scoop_voxel
                        .and_then(|v| self.liquid_level.get(&v).copied())
                        .unwrap_or(0);
                    let scoop_is_source =
                        scoop_voxel.is_some() && scoop_level == 0;
                    if scoop_is_source
                        && let (Some(filled), Some(voxel)) = (
                            liquid_to_bucket_id(scoop_block.0),
                            scoop_voxel,
                        )
                    {
                        self.world_set_tracked(voxel, BlockId::AIR);
                        let coords = Self::affected_chunks(voxel);
                        self.rebuild_chunks(&coords);
                        self.inventory[self.selected_slot] =
                            Some((BlockId(filled), 1));
                        let edit = EditMsg {
                            x: voxel[0],
                            y: voxel[1],
                            z: voxel[2],
                            block: 0,
                        };
                        match &mut self.net {
                            NetRole::Offline => {}
                            NetRole::Server { server, .. } => {
                                self.edit_history.push(edit);
                                if let Ok(buf) =
                                    bincode::serde::encode_to_vec(
                                        edit, cfg,
                                    )
                                {
                                    server.broadcast_bytes(
                                        "vx.edit", buf,
                                    );
                                }
                            }
                            NetRole::Client {
                                pending_edits, ..
                            } => {
                                pending_edits.push(edit);
                            }
                        }
                        let center = [
                            voxel[0] as f32 + 0.5,
                            voxel[1] as f32 + 0.5,
                            voxel[2] as f32 + 0.5,
                        ];
                        self.sfx_at("splash", center, |a, g| {
                            let _ = a.play_thud(140.0, 0.18 * g);
                            let _ = a.play_swish(0.16 * g);
                        });
                    }
                } else if let Some(liquid_id) = bucket_liquid_id(held) {
                    let target_block = self.world.get(hit.voxel);
                    let target_voxel = if block_is_replaceable(target_block) {
                        hit.voxel
                    } else {
                        hit.place_at
                    };
                    let dest = self.world.get(target_voxel);
                    if dest.is_air() || block_is_replaceable(dest) {
                        self.world_set_tracked(
                            target_voxel,
                            BlockId(liquid_id),
                        );
                        self.liquid_level.insert(target_voxel, 0);
                        let coords = Self::affected_chunks(target_voxel);
                        self.rebuild_chunks(&coords);
                        self.inventory[self.selected_slot] =
                            Some((BlockId(BUCKET_EMPTY_ID), 1));
                        let edit = EditMsg {
                            x: target_voxel[0],
                            y: target_voxel[1],
                            z: target_voxel[2],
                            block: liquid_id,
                        };
                        match &mut self.net {
                            NetRole::Offline => {}
                            NetRole::Server { server, .. } => {
                                self.edit_history.push(edit);
                                if let Ok(buf) =
                                    bincode::serde::encode_to_vec(
                                        edit, cfg,
                                    )
                                {
                                    server.broadcast_bytes(
                                        "vx.edit", buf,
                                    );
                                }
                                if let Ok(buf) =
                                    bincode::serde::encode_to_vec(
                                        (target_voxel, 0u8),
                                        cfg,
                                    )
                                {
                                    server.broadcast_bytes(
                                        "vx.liquid_lvl",
                                        buf,
                                    );
                                }
                            }
                            NetRole::Client {
                                pending_edits, ..
                            } => {
                                pending_edits.push(edit);
                            }
                        }
                        let center = [
                            target_voxel[0] as f32 + 0.5,
                            target_voxel[1] as f32 + 0.5,
                            target_voxel[2] as f32 + 0.5,
                        ];
                        self.sfx_at("splash", center, |a, g| {
                            let _ = a.play_thud(120.0, 0.18 * g);
                            let _ = a.play_swish(0.14 * g);
                        });
                    }
                }
            } else if rmb
                && let Some(block) = selected_block
                && !is_tool_block(block)
            {
                // Pick the destination voxel: if the crosshair is
                // on a `F_REPLACEABLE` cell (water, lava, snow
                // layer), the new block lands AT that cell -
                // mirroring MC where a block dropped into water
                // displaces the water. Otherwise the usual
                // "adjacent to the hit face" cell applies.
                let target_voxel = if block_is_replaceable(
                    self.world.get(hit.voxel),
                ) && self.world.get(hit.voxel).0 != block.0
                {
                    hit.voxel
                } else {
                    hit.place_at
                };
                // Reject placement when it would intersect the
                // player capsule - otherwise the player gets stuck
                // inside the freshly placed block.
                let (min, max) = body_aabb(self.player_pos);
                let p = target_voxel;
                let voxel_min = [p[0] as f32, p[1] as f32, p[2] as f32];
                let voxel_max = [p[0] as f32 + 1.0, p[1] as f32 + 1.0, p[2] as f32 + 1.0];
                let overlaps = max.x > voxel_min[0]
                    && min.x < voxel_max[0]
                    && max.y > voxel_min[1]
                    && min.y < voxel_max[1]
                    && max.z > voxel_min[2]
                    && min.z < voxel_max[2];
                // Block-occupied check: any non-air voxel marked
                // `F_NO_PLACE` (torch, flowers, ...) blocks RMB
                // placement so the player has to break the
                // decoration first. Replaceable voxels (liquids,
                // snow layer) are explicitly NOT occupied.
                let target_block = self.world.get(target_voxel);
                let occupied = block_is_placement_blocked(target_block)
                    || (!target_block.is_air()
                        && !block_is_replaceable(target_block));
                // Torches need at least one solid neighbour
                // (floor or wall) to mount on. A neighbour that's
                // air OR another `F_NO_PLACE` decoration doesn't
                // count as support - torches can't hang off
                // torches / flowers.
                let torch_unsupported = block.0 == TORCH_ID && {
                    let q = target_voxel;
                    let unsupportive = |b: BlockId| {
                        b.is_air() || block_is_placement_blocked(b)
                    };
                    let below_blk = self.world.get([q[0], q[1] - 1, q[2]]);
                    let below_air = unsupportive(below_blk);
                    let walls_clear = [
                        [q[0] - 1, q[1], q[2]],
                        [q[0] + 1, q[1], q[2]],
                        [q[0], q[1], q[2] - 1],
                        [q[0], q[1], q[2] + 1],
                    ]
                    .iter()
                    .all(|v| unsupportive(self.world.get(*v)));
                    below_air && walls_clear
                };
                if !overlaps && !torch_unsupported && !occupied {
                    let edit = EditMsg {
                        x: target_voxel[0],
                        y: target_voxel[1],
                        z: target_voxel[2],
                        block: block.0,
                    };
                    // Record sticky torch orientation from the
                    // ray hit normal so a later neighbour change
                    // can't flip a floor torch into a wall torch.
                    let torch_orient_msg: Option<([i32; 3], u8)> =
                        if block.0 == TORCH_ID {
                            let n = hit.normal;
                            let orient = if n[1] > 0 {
                                0u8
                            } else if n[0] > 0 {
                                1
                            } else if n[0] < 0 {
                                2
                            } else if n[2] > 0 {
                                3
                            } else if n[2] < 0 {
                                4
                            } else {
                                0
                            };
                            self.torch_orient
                                .insert(target_voxel, orient);
                            Some((target_voxel, orient))
                        } else {
                            None
                        };
                    if is_client {
                        self.world_set_tracked(target_voxel, block);
                        let coords = Self::affected_chunks(target_voxel);
                        self.rebuild_chunks(&coords);
                        if let NetRole::Client { client, pending_edits, .. } = &mut self.net {
                            pending_edits.push(edit);
                            // Ship the torch orientation
                            // alongside the place so the host +
                            // every other peer renders the right
                            // lean direction.
                            if let Some(payload) = torch_orient_msg
                                && let Ok(buf) =
                                    bincode::serde::encode_to_vec(
                                        payload,
                                        bincode::config::standard(),
                                    )
                            {
                                let _ = client
                                    .send_bytes("vx.torch_orient", buf);
                            }
                        }
                    } else {
                        self.world_set_tracked(target_voxel, block);
                        let coords = Self::affected_chunks(target_voxel);
                        self.rebuild_chunks(&coords);
                        if let NetRole::Server { server, .. } = &self.net {
                            let cfg = bincode::config::standard();
                            if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                                server.broadcast_bytes("vx.edit", buf);
                            }
                            // Broadcast the host's torch lean so
                            // remote clients render the matching
                            // wall mount.
                            if let Some(payload) = torch_orient_msg
                                && let Ok(buf) =
                                    bincode::serde::encode_to_vec(
                                        payload, cfg,
                                    )
                            {
                                server.broadcast_bytes(
                                    "vx.torch_orient",
                                    buf,
                                );
                            }
                        }
                        self.edit_history.push(edit);
                    }
                    // Chest place: mint an empty 27-slot container
                    // at the placed voxel so a follow-up RMB open
                    // sees a clean state. Host-authoritative; a
                    // client's local copy gets the empty container
                    // too so the offline path works, but the wire
                    // sync of mutations is host-driven.
                    if block.0 == CHEST_ID {
                        self.chests
                            .entry(target_voxel)
                            .or_insert_with(block::chest::new_chest);
                    }
                    // Furnace place: mint an empty 3-slot container
                    // + a zeroed FurnaceRuntime entry. Same idempotent
                    // shape as the chest path (entry().or_insert_with);
                    // the live cook tick advances the runtime each
                    // frame, save / load round trip preserves both.
                    if block.0 == FURNACE_ID {
                        block::furnace::place_furnace(
                            &mut self.furnaces,
                            &mut self.furnace_runtimes,
                            target_voxel,
                        );
                    }
                    // Decrement the active stack now that placement
                    // succeeded. Clients defer this - the server's
                    // `vx.inv` snapshot reflects the decrement.
                    if !is_client
                        && let Some((_, count)) =
                            self.inventory[self.selected_slot].as_mut()
                    {
                        *count = count.saturating_sub(1);
                        if *count == 0 {
                            self.inventory[self.selected_slot] = None;
                        }
                    }
                    // Place sound: reuse the same `dig:*` material
                    // bank (MC does the same - the place sound IS
                    // the dig sound), just played at the placement
                    // voxel with the destination block's material.
                    let key = format!("dig:{}", block_material(block));
                    let freq = 380.0 + (block.0 as f32) * 22.0;
                    let place_center = [
                        target_voxel[0] as f32 + 0.5,
                        target_voxel[1] as f32 + 0.5,
                        target_voxel[2] as f32 + 0.5,
                    ];
                    self.sfx_at(&key, place_center, move |a, g| {
                        let _ = a.play_click(freq, 0.18 * g);
                    });
                    // Albedo-tinted dust puff so the place reads
                    // visually too.
                    let color = self
                        .registry
                        .get(block)
                        .map(|d| d.color)
                        .unwrap_or([0.7, 0.7, 0.7]);
                    self.spawn_place_particles(target_voxel, color);
                    self.blocks_placed =
                        self.blocks_placed.saturating_add(1);
                    self.place_history.push(target_voxel);
                    if self.place_history.len() > 32 {
                        let drop = self.place_history.len() - 32;
                        self.place_history.drain(0..drop);
                    }
                }
            }
        }
        // Net tick: poll + apply + broadcast / send.
        self.tick_net();
        // Authoritative side simulates wandering mobs. The render
        // path reads `self.remote_mobs` which gets populated by
        // both `tick_mobs` (server) and `vx.mob` (client).
        self.tick_mobs(dt);
        // Generic liquid physics: drive each registered liquid
        // (water, lava, ...) at its own MC-style cadence. Per
        // liquid the host runs fall -> spread -> optional source
        // merge -> decay. Clients skip and rely on `vx.edit`.
        self.tick_liquids();
        // Step every floating drop entity: gravity, bob,
        // pickup-on-touch, despawn after lifetime. Host runs
        // the full sim; clients also tick locally for smooth
        // motion between server snapshots.
        self.tick_drops(dt);
        // Live furnace cook step. Walks every furnace + advances
        // fuel + cook progress by `dt_ms` (the frame delta in
        // milliseconds). The offline catch-up path in
        // `world_save::timers::catch_up_furnace` handles the
        // window between save and reload.
        let dt_ms = (dt * 1000.0).round().max(0.0) as u32;
        block::furnace::furnace_tick(
            &mut self.furnaces,
            &mut self.furnace_runtimes,
            dt_ms,
        );
        // Phase F.6: portal-overlap counter + cross-dim teleport.
        self.tick_player_portal();
        // Auto-close the chest UI when the player walks more than
        // ~5 blocks from the open chest. Matches MC's "stay close
        // to the container" rule. Done here (per-frame) so the
        // close fires even while no input lands.
        if let Some(pos) = self.chest_open {
            let dx = self.player_pos.x - (pos[0] as f32 + 0.5);
            let dy = self.player_pos.y - (pos[1] as f32 + 0.5);
            let dz = self.player_pos.z - (pos[2] as f32 + 0.5);
            let dist_sq = dx * dx + dy * dy + dz * dz;
            const CHEST_MAX_DIST: f32 = 5.0;
            if dist_sq > CHEST_MAX_DIST * CHEST_MAX_DIST {
                self.close_chest_ui();
            }
        }
        // Same 5-block leash for the furnace UI. Mirrors the
        // chest-close check above so walking away closes the panel
        // and routes the held stack back to the furnace (or to
        // floor drops if nothing fits there).
        if let Some(pos) = self.furnace_open {
            let dx = self.player_pos.x - (pos[0] as f32 + 0.5);
            let dy = self.player_pos.y - (pos[1] as f32 + 0.5);
            let dz = self.player_pos.z - (pos[2] as f32 + 0.5);
            let dist_sq = dx * dx + dy * dy + dz * dz;
            const FURNACE_MAX_DIST: f32 = 5.0;
            if dist_sq > FURNACE_MAX_DIST * FURNACE_MAX_DIST {
                self.close_furnace_ui();
            }
        }
        // Mob audio runs on every peer (host + each client): step
        // sting per moving sheep, occasional baa, both
        // distance-attenuated through `sfx_at`.
        self.tick_mob_audio(dt);
        // Tick particles regardless of net role.
        self.particles.tick(dt);
        // Tick the day clock unconditionally - even paused / in
        // menus the world keeps turning so the host's lighting is
        // never out of sync with what a client sees. /freezetime
        // pauses the advance.
        if !self.freeze_time {
            self.day_time = (self.day_time + dt).rem_euclid(DAY_LENGTH_S);
        }
        // Day <-> night transition announcer. Fires once per
        // crossing using the sun elevation so the chat doesn't
        // spam every frame.
        let phase_t = (self.day_time / DAY_LENGTH_S).rem_euclid(1.0);
        let elev_t = (phase_t * std::f32::consts::TAU).sin();
        let is_night_now = elev_t < -0.05;
        if is_night_now != self.was_night {
            let line = if is_night_now {
                "* night falls"
            } else {
                "* dawn breaks"
            };
            self.chat_log.push((
                std::time::Instant::now(),
                line.to_string(),
            ));
            self.was_night = is_night_now;
        }

        // Rebuild the combined water mesh once the dirty flag is
        // set. Done before the renderer borrow below so the
        // create_mesh / remove_mesh churn doesn't fight with the
        // per-frame draw calls.
        self.maybe_rebuild_liquid_mesh();
        let player_pos = self.player_pos;
        let chunk_meshes = self.chunk_meshes.clone();
        let registry = &self.registry;
        // Wireframe outline follows the mining ray so the player
        // sees the highlight on a decoration (torch / flower)
        // that they can break, not on the solid block hidden
        // behind it. Falls back to the placement target when no
        // mineable cell is in view (shouldn't happen in
        // practice since `mine_target` matches every non-air
        // voxel).
        let target = self.mine_target.or(self.target);
        let ui = self.ui.as_mut();
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        {
            let cam = renderer.camera_3d_mut();
            // Refresh global shader time. Drives the animated
            // water surface (caustic noise scrolls along the
            // baked flow direction).
            cam.time_seconds = self.app_start.elapsed().as_secs_f32();
            // Head-bob offset: a small vertical bob + lateral
            // sway scaled by current horizontal speed so a still
            // player has a rock-steady camera. Skip while in fly
            // mode - hovering shouldn't shake the view.
            let h_speed = (self.player_vel.x * self.player_vel.x
                + self.player_vel.z * self.player_vel.z)
                .sqrt();
            let bob_amount =
                if !self.settings.head_bob || self.fly_mode || !self.grounded {
                    0.0
                } else {
                    (h_speed / PLAYER_SPEED).clamp(0.0, 1.5)
                };
            // Sprint widens the bob a little (still subtle) so
            // running feels heavier than walking.
            let sprint_boost = if self.sprinting { 1.6 } else { 1.0 };
            let bob_y =
                (self.head_bob_phase * 2.0).sin() * 0.05 * bob_amount * sprint_boost;
            let bob_x =
                self.head_bob_phase.sin() * 0.03 * bob_amount * sprint_boost;
            let right = Vec3::new(
                -(-self.player_yaw.sin()),
                0.0,
                -self.player_yaw.cos(),
            )
            .normalize_or_zero();
            let bob = Vec3::new(right.x * bob_x, bob_y, right.z * bob_x);
            // Third-person modes pull the camera back along the
            // view ray (mode 1) or push it in front of the player
            // looking backward (mode 2). The offset is short
            // enough that it usually stays inside a 1-chunk view
            // bubble even on tight walls.
            const TP_DIST: f32 = 4.5;
            match self.camera_mode {
                0 => {
                    cam.position = player_pos + bob;
                    cam.target = player_pos + bob + forward;
                }
                1 => {
                    cam.position = player_pos + bob - forward * TP_DIST;
                    cam.target = player_pos + bob;
                }
                _ => {
                    cam.position = player_pos + bob + forward * TP_DIST;
                    cam.target = player_pos + bob;
                }
            }
            cam.fov_y_rad = self.fov_y_rad;
        }
        // Day/night cycle. `day_time` ramps from 0..DAY_LENGTH_S
        // and feeds both the sun direction (rotating around the +Z
        // axis) and the clear color (sunrise -> noon -> sunset ->
        // night). The sky color is a perceptual mix between three
        // anchor swatches keyed on the sun's elevation.
        let (sun_dir, mut sky) = day_night(self.day_time);
        // Precipitation re-tints the sky. Rain: dark stormy
        // grey. Snow: lighter, slightly blue-tinged overcast.
        if self.weather == 1 {
            let target = [0.42, 0.46, 0.52];
            let t = 0.6;
            sky[0] = sky[0] * (1.0 - t) + target[0] * t;
            sky[1] = sky[1] * (1.0 - t) + target[1] * t;
            sky[2] = sky[2] * (1.0 - t) + target[2] * t;
        } else if self.weather == 2 {
            let target = [0.70, 0.74, 0.80];
            let t = 0.55;
            sky[0] = sky[0] * (1.0 - t) + target[0] * t;
            sky[1] = sky[1] * (1.0 - t) + target[1] * t;
            sky[2] = sky[2] * (1.0 - t) + target[2] * t;
        }
        // Active lightning flash washes the sky toward white.
        if self.lightning_flash > 0.0 {
            let t = (self.lightning_flash / 0.18).clamp(0.0, 1.0);
            let target = [1.0, 1.0, 1.0];
            sky[0] = sky[0] * (1.0 - t) + target[0] * t;
            sky[1] = sky[1] * (1.0 - t) + target[1] * t;
            sky[2] = sky[2] * (1.0 - t) + target[2] * t;
        }
        renderer.set_sun_dir(sun_dir);
        renderer.set_clear_color(sky[0] as f64, sky[1] as f64, sky[2] as f64, 1.0);
        // Tie sun intensity to the real (un-clamped) elevation so
        // night is actually dim. We recompute the raw direction
        // here because `day_night` clamps the dir vector when the
        // sun is below the horizon (to keep shadows sane), losing
        // the sign we need for the intensity curve.
        let phase = (self.day_time / DAY_LENGTH_S).rem_euclid(1.0);
        let theta = phase * std::f32::consts::TAU;
        let raw_elev = theta.sin();
        // 3.0 at noon, ~0.4 at midnight (soft moonlight), smooth
        // transition through the horizon. Clamp avoids negatives.
        let sun_intensity = (1.5 + raw_elev * 1.5).clamp(0.10, 3.0);
        renderer.set_sun_intensity(sun_intensity);
        // Sun + moon as 3D meshes far away. Going through the
        // mesh3d pass means the depth buffer occludes them when
        // terrain stands between the player + the sky (rooms,
        // mountains, etc.). We over-saturate the albedo (>1.0) so
        // the PBR shader's `direct + ambient` output clamps to
        // bright on every face - no need for a dedicated unlit
        // pipeline yet.
        let player_cube_handle =
            *self.player_cube.get_or_insert_with(|| renderer.cube_handle());
        const CELESTIAL_DIST: f32 = 140.0;
        // Sun: only render while above the horizon (matches MC
        // exactly + avoids a fake-glow disk piercing the floor
        // at night). Smooth alpha gate uses the same elevation
        // curve as the sky color.
        let sun_elev = sun_dir.y;
        if sun_elev > -0.05 {
            let sun_world = Vec3::new(
                player_pos.x + sun_dir.x * CELESTIAL_DIST,
                player_pos.y + sun_dir.y * CELESTIAL_DIST,
                player_pos.z + sun_dir.z * CELESTIAL_DIST,
            );
            let m = Mat4::from_scale_rotation_translation(
                Vec3::splat(14.0),
                glam::Quat::IDENTITY,
                sun_world,
            );
            renderer.draw_mesh_pbr(
                player_cube_handle,
                m.to_cols_array(),
                [5.0, 4.5, 2.2, 1.0],
                0.0,
                1.0,
            );
        }
        // Moon: opposite of sun, slightly smaller + cooler tint.
        let moon_dir = -sun_dir;
        if moon_dir.y > -0.05 {
            let moon_world = Vec3::new(
                player_pos.x + moon_dir.x * CELESTIAL_DIST,
                player_pos.y + moon_dir.y * CELESTIAL_DIST,
                player_pos.z + moon_dir.z * CELESTIAL_DIST,
            );
            let m = Mat4::from_scale_rotation_translation(
                Vec3::splat(10.0),
                glam::Quat::IDENTITY,
                moon_world,
            );
            renderer.draw_mesh_pbr(
                player_cube_handle,
                m.to_cols_array(),
                [2.8, 3.0, 3.8, 1.0],
                0.0,
                1.0,
            );
        }
        // Push the 8 nearest torch positions to the renderer as
        // point lights. We sort by squared distance so the player
        // always sees the closest sources lit; far torches just
        // drop out (MAX_LIGHTS = 8 cap in the shader).
        let mut torches: Vec<([i32; 3], f32)> = self
            .torch_positions
            .iter()
            .map(|p| {
                let dx = p[0] as f32 + 0.5 - player_pos.x;
                let dy = p[1] as f32 + 0.5 - player_pos.y;
                let dz = p[2] as f32 + 0.5 - player_pos.z;
                (*p, dx * dx + dy * dy + dz * dz)
            })
            .collect();
        torches.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        for (p, _) in torches.iter().take(8) {
            // Subtle flicker on each light's intensity hashed by
            // position so the room reads as living torchlight,
            // not a static lamp.
            let phase = (p[0] as u32)
                .wrapping_mul(2654435761)
                ^ ((p[1] as u32).wrapping_mul(0x9E37_79B1))
                ^ ((p[2] as u32).wrapping_mul(0x85EB_CA6B));
            let phase_f =
                (phase as f32 / u32::MAX as f32) * std::f32::consts::TAU;
            let flicker = (self.day_time * 8.0 + phase_f).sin();
            let intensity = 2.7 + 0.4 * flicker;
            renderer.queue_point_light(
                [p[0] as f32 + 0.5, p[1] as f32 + 0.5, p[2] as f32 + 0.5],
                [1.0, 0.78, 0.40],
                intensity,
                8.0,
            );
        }
        // Render every torch as a thin wooden pillar + a small
        // glowing tip. Orientation is derived from the world
        // each frame: if the voxel directly below is solid the
        // torch stands upright; otherwise it leans toward the
        // first neighbouring solid block (wall-mount).
        if !self.torch_positions.is_empty() {
            let cube = *self
                .player_cube
                .get_or_insert_with(|| renderer.cube_handle());
            for p in self.torch_positions.iter() {
                // Pick orientation:
                //   (dx, dz, lean angle around X/Z, tilt_dir):
                //   (0, 0) = floor torch.
                let below = self.world.get([p[0], p[1] - 1, p[2]]);
                let neighbours = [
                    ([p[0] - 1, p[1], p[2]], (-1, 0)),
                    ([p[0] + 1, p[1], p[2]], (1, 0)),
                    ([p[0], p[1], p[2] - 1], (0, -1)),
                    ([p[0], p[1], p[2] + 1], (0, 1)),
                ];
                let (lean_x, lean_z) = if let Some(orient) =
                    self.torch_orient.get(p).copied()
                {
                    match orient {
                        1 => (1, 0),
                        2 => (-1, 0),
                        3 => (0, 1),
                        4 => (0, -1),
                        _ => (0, 0),
                    }
                } else if !below.is_air() {
                    (0i32, 0i32)
                } else {
                    let mut found = (0, 0);
                    for (v, (dx, dz)) in neighbours.iter() {
                        if !self.world.get(*v).is_air() {
                            found = (-dx, -dz);
                            break;
                        }
                    }
                    found
                };
                let cx = p[0] as f32 + 0.5;
                let cy = p[1] as f32;
                let cz = p[2] as f32 + 0.5;
                // Build a transform: floor torch points up;
                // wall torch tilts ~30 deg toward the lean dir
                // + shifts toward the supporting wall.
                let (stick_pos, stick_rot, tip_pos) =
                    if lean_x == 0 && lean_z == 0 {
                        (
                            Vec3::new(cx, cy + 0.35, cz),
                            glam::Quat::IDENTITY,
                            Vec3::new(cx, cy + 0.78, cz),
                        )
                    } else {
                        let tilt_axis = if lean_x != 0 {
                            Vec3::Z
                        } else {
                            Vec3::X
                        };
                        let tilt_sign = if lean_x != 0 {
                            -lean_x as f32
                        } else {
                            lean_z as f32
                        };
                        let angle = 0.55 * tilt_sign;
                        let rot = glam::Quat::from_axis_angle(
                            tilt_axis, angle,
                        );
                        // Shift the torch base toward the wall
                        // it's mounted on so the foot sits on
                        // the supporting block.
                        let shift_x = -lean_x as f32 * 0.42;
                        let shift_z = -lean_z as f32 * 0.42;
                        let sx = cx + shift_x;
                        let sz = cz + shift_z;
                        let sy = cy + 0.45;
                        let tip_x = sx
                            + lean_x as f32 * 0.18;
                        let tip_z = sz
                            + lean_z as f32 * 0.18;
                        let tip_y = sy + 0.32;
                        (
                            Vec3::new(sx, sy, sz),
                            rot,
                            Vec3::new(tip_x, tip_y, tip_z),
                        )
                    };
                let stick = Mat4::from_scale_rotation_translation(
                    Vec3::new(0.12, 0.7, 0.12),
                    stick_rot,
                    stick_pos,
                );
                renderer.draw_mesh_pbr(
                    cube,
                    stick.to_cols_array(),
                    [0.55, 0.40, 0.20, 1.0],
                    0.0,
                    0.9,
                );
                // Flame tip: small emissive-ish orange cube
                // with an id-hashed flicker so neighbouring
                // torches don't pulse in lockstep.
                let phase = (p[0] as u32)
                    .wrapping_mul(2654435761)
                    ^ ((p[1] as u32).wrapping_mul(0x9E37_79B1))
                    ^ ((p[2] as u32).wrapping_mul(0x85EB_CA6B));
                let phase_f =
                    (phase as f32 / u32::MAX as f32) * std::f32::consts::TAU;
                let flicker_t =
                    (self.day_time * 10.0 + phase_f).sin();
                let scale = 0.16 + 0.025 * flicker_t;
                let tip_pos = Vec3::new(
                    tip_pos.x,
                    tip_pos.y + 0.015 * flicker_t,
                    tip_pos.z,
                );
                let r = 1.0;
                let g = (0.72 + 0.10 * flicker_t).clamp(0.0, 1.0);
                let b = (0.25 + 0.08 * flicker_t).clamp(0.0, 1.0);
                let tip = Mat4::from_scale_rotation_translation(
                    Vec3::splat(scale),
                    glam::Quat::IDENTITY,
                    tip_pos,
                );
                renderer.draw_mesh_pbr(
                    cube,
                    tip.to_cols_array(),
                    [r, g, b, 1.0],
                    0.0,
                    0.4,
                );
            }
        }
        // Snow layers: thin 1x0.12x1 slab covering the bottom
        // of the voxel cell. Atlas-tinted near-white so it
        // contrasts with whatever sits below.
        if !self.snow_positions.is_empty() {
            let cube = *self
                .player_cube
                .get_or_insert_with(|| renderer.cube_handle());
            for (p, layers) in self.snow_positions.iter() {
                let cx = p[0] as f32 + 0.5;
                let cy = p[1] as f32;
                let cz = p[2] as f32 + 0.5;
                let h = (*layers as f32 / 8.0).clamp(0.06, 1.0);
                let slab = Mat4::from_scale_rotation_translation(
                    Vec3::new(1.0, h, 1.0),
                    glam::Quat::IDENTITY,
                    Vec3::new(cx, cy + h * 0.5, cz),
                );
                renderer.draw_mesh_pbr(
                    cube,
                    slab.to_cols_array(),
                    [0.97, 0.98, 1.0, 1.0],
                    0.0,
                    0.9,
                );
            }
        }
        // Liquids: one slope-aware mesh per kind (water + lava +
        // future). Each draws with its own albedo from the
        // matching `BlockType` row, so adding a new liquid means
        // a single table flip rather than another draw call here.
        for (liquid_id, handle) in &self.liquid_mesh_handles {
            let Some(bt) = block_type(*liquid_id) else {
                continue;
            };
            // Per-liquid alpha + roughness: water reads more
            // translucent + smooth, lava nearly opaque + matte.
            let (alpha, rough) = match *liquid_id {
                WATER_ID => (0.65_f32, 0.18_f32),
                LAVA_ID => (0.92, 0.55),
                _ => (0.75, 0.30),
            };
            // Water gets the animated caustic-ripple shader path;
            // lava (no flow texture authored yet) keeps the plain
            // PBR path. Both will eventually share a "flowing
            // surface" pipeline.
            if *liquid_id == WATER_ID {
                renderer.draw_mesh_water(
                    *handle,
                    Mat4::IDENTITY.to_cols_array(),
                    [bt.color[0], bt.color[1], bt.color[2], alpha],
                    0.0,
                    rough,
                );
            } else {
                renderer.draw_mesh_pbr(
                    *handle,
                    Mat4::IDENTITY.to_cols_array(),
                    [bt.color[0], bt.color[1], bt.color[2], alpha],
                    0.0,
                    rough,
                );
            }
        }
        // Floating drop entities. Each renders as a small cube
        // (~0.3 voxel) bobbing in place. Pulled off
        // `dropped_items` so both host + client see the same
        // set; bob phase comes from the drop's `age` so
        // neighbouring items don't oscillate in lockstep.
        if !self.dropped_items.is_empty() {
            let cube = *self
                .player_cube
                .get_or_insert_with(|| renderer.cube_handle());
            let chest_y = player_pos.y - PLAYER_HEIGHT * 0.4;
            for drop in self.dropped_items.iter() {
                let bob = (drop.age * 2.4).sin() * 0.08;
                let block = BlockId(drop.block);
                let color = self
                    .registry
                    .get(block)
                    .map(|d| d.color)
                    .unwrap_or([0.75, 0.55, 0.30]);
                // Magnet visual: when the drop is in the attract
                // band, shrink it toward the player + spin faster
                // so the pickup reads as a satisfying suck-in
                // animation. Outside the band the drop sits at
                // base scale + spin.
                let dx = player_pos.x - drop.pos[0];
                let dy = chest_y - drop.pos[1];
                let dz = player_pos.z - drop.pos[2];
                let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                let attract = if dist < DROP_ATTRACT_RADIUS && dist > 1e-3 {
                    (1.0 - dist / DROP_ATTRACT_RADIUS).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                // Scale ramps from 0.28 (resting) down to 0.08
                // at the collect band. Spin speed bumps with
                // attract so the cube wobbles into the player.
                let base_scale = 0.28;
                let scale = base_scale * (1.0 - 0.7 * attract);
                let spin_rate = 1.4 + 6.0 * attract;
                // Lean toward the player by a fraction of the
                // delta so the drop physically streaks in.
                let lean = 0.45 * attract;
                let render_pos = Vec3::new(
                    drop.pos[0] + dx * lean,
                    drop.pos[1] + 0.14 + bob + dy * lean,
                    drop.pos[2] + dz * lean,
                );
                let m = Mat4::from_scale_rotation_translation(
                    Vec3::splat(scale),
                    glam::Quat::from_rotation_y(drop.age * spin_rate),
                    render_pos,
                );
                renderer.draw_mesh_pbr(
                    cube,
                    m.to_cols_array(),
                    [color[0], color[1], color[2], 1.0],
                    0.0,
                    0.7,
                );
            }
        }
        // Render remote players as coloured cubes so each peer is
        // visible. The interpolation buffer (`remote_interp`) is
        // populated by every received `vx.player` regardless of
        // role, so a single sample call here covers Server + Client
        // paths uniformly. We skip our own id (0 for the host, the
        // assigned id for a client) to avoid rendering ourselves.
        let now = std::time::Instant::now();
        let own_id: u32 = match &self.net {
            NetRole::Offline => u32::MAX,
            NetRole::Server { .. } => 0,
            NetRole::Client { client, .. } => client.client_id.unwrap_or(0),
        };
        let remote_iter: Vec<PlayerMsg> = if matches!(self.net, NetRole::Offline) {
            Vec::new()
        } else {
            self.remote_interp
                .iter()
                .filter(|(id, _)| **id != own_id)
                .map(|(_, r)| r.sample(now))
                .collect()
        };
        // In third-person we draw the local player too so the
        // pulled-back camera has something to look at. We
        // synthesize a PlayerMsg from current state and prepend
        // it to the render list.
        let mut local_iter = remote_iter;
        if self.camera_mode != 0 {
            local_iter.push(PlayerMsg {
                id: own_id,
                pos: [player_pos.x, player_pos.y, player_pos.z],
                yaw: self.player_yaw,
                pitch: self.player_pitch,
            });
        }
        if !local_iter.is_empty() {
            let cube = self.player_cube.get_or_insert_with(|| renderer.cube_handle());
            for p in local_iter {
                // Hash id to a stable colour + a slightly cooler
                // head tone so the two parts read as separate body
                // segments instead of a single block.
                let h = (p.id.wrapping_mul(2654435761)) as f32 / u32::MAX as f32;
                let tau = std::f32::consts::TAU;
                let body_color = [
                    (0.4 + 0.5 * (h * tau).sin().abs()).min(1.0),
                    (0.4 + 0.5 * ((h + 0.33) * tau).sin().abs()).min(1.0),
                    (0.4 + 0.5 * ((h + 0.66) * tau).sin().abs()).min(1.0),
                    1.0,
                ];
                let head_color = [
                    (body_color[0] * 0.85).min(1.0),
                    (body_color[1] * 0.85).min(1.0),
                    (body_color[2] * 0.85).min(1.0),
                    1.0,
                ];
                let rot = glam::Quat::from_rotation_y(p.yaw);
                let body = Mat4::from_scale_rotation_translation(
                    Vec3::new(0.65, 1.0, 0.4),
                    rot,
                    Vec3::new(p.pos[0], p.pos[1] - 1.2, p.pos[2]),
                );
                renderer.draw_mesh_pbr(*cube, body.to_cols_array(), body_color, 0.0, 0.7);
                let head = Mat4::from_scale_rotation_translation(
                    Vec3::new(0.55, 0.55, 0.55),
                    rot,
                    Vec3::new(p.pos[0], p.pos[1] - 0.45, p.pos[2]),
                );
                renderer.draw_mesh_pbr(*cube, head.to_cols_array(), head_color, 0.0, 0.7);
            }
        }
        // Render every known sheep (server: from `mobs`, client:
        // from `remote_mobs` populated by `vx.mob`). Sheep are
        // boxy white cubes with a small head cube on top for
        // silhouette. Scaled wider than tall so they read as
        // four-legged rather than human-shaped.
        if !self.remote_mobs.is_empty() {
            let player_cube = self
                .player_cube
                .get_or_insert_with(|| renderer.cube_handle());
            // Body uses the dedicated textured sheep mesh when
            // available; head always uses the plain cube mesh
            // tinted skin-tone so the two parts can have different
            // albedos without needing two separate textured
            // meshes (the MC sheep model has a separate head
            // material too).
            let body_mesh = self.sheep_mesh.unwrap_or(*player_cube);
            let body_tint: [f32; 4] = if self.sheep_mesh.is_some() {
                // White tint = let the wool texture show through
                // unmodulated.
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [0.92, 0.92, 0.92, 1.0]
            };
            // Head: prefer the textured `sheep_head_mesh` (cropped
            // face region of the MC entity texture) so the front
            // shows a real sheep face. Falls back to the
            // untextured player_cube with a skin tint when the
            // crop wasn't loaded.
            let head_mesh = self.sheep_head_mesh.unwrap_or(*player_cube);
            let head_color: [f32; 4] = if self.sheep_head_mesh.is_some() {
                [1.0, 1.0, 1.0, 1.0]
            } else {
                [0.92, 0.86, 0.78, 1.0]
            };
            let mobs_snapshot: Vec<MobMsg> =
                self.remote_mobs.values().copied().collect();
            for mob in mobs_snapshot {
                let hurt_flash = self
                    .mob_hurt_flash
                    .get(&mob.id)
                    .copied()
                    .unwrap_or(0.0)
                    .clamp(0.0, 1.0);
                // Death-fade progress in `[0, 1]`. Zero = alive.
                let dying_t = self
                    .mob_dying
                    .get(&mob.id)
                    .copied()
                    .unwrap_or(0.0);
                let death_progress = if dying_t > 0.0 {
                    ((MOB_DEATH_FADE - dying_t) / MOB_DEATH_FADE).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let body_brightness = 1.0 - 0.55 * death_progress;
                let tilt = glam::Quat::from_rotation_z(
                    -std::f32::consts::FRAC_PI_2 * death_progress,
                );
                let death_sink = 0.18 * death_progress;
                // Per-kind body proportions + meshes + base colour.
                // Sheep uses the textured sheep_body + sheep_head
                // meshes; zombies use the plain player_cube with a
                // dim green tint stretched into a humanoid box.
                let is_zombie = mob.kind == KIND_ZOMBIE;
                let is_chicken = mob.kind == KIND_CHICKEN;
                let is_pig = mob.kind == KIND_PIG;
                let (body_scale, body_y_off, base_body_tint, body_use_mesh) = if is_zombie {
                    (
                        Vec3::new(0.65, 1.55, 0.45),
                        0.78,
                        [0.30, 0.55, 0.30, 1.0],
                        player_cube_handle,
                    )
                } else if is_chicken {
                    (
                        Vec3::new(0.45, 0.55, 0.4),
                        0.28,
                        [0.95, 0.95, 0.9, 1.0],
                        player_cube_handle,
                    )
                } else if is_pig {
                    // Pinkish stretched cube.
                    (
                        Vec3::new(1.1, 0.75, 0.6),
                        0.35,
                        [0.95, 0.65, 0.65, 1.0],
                        player_cube_handle,
                    )
                } else {
                    (
                        Vec3::new(1.2, 0.8, 0.7),
                        0.4,
                        body_tint,
                        body_mesh,
                    )
                };
                let tint = [
                    (base_body_tint[0] * (1.0 - hurt_flash) + 1.0 * hurt_flash)
                        * body_brightness,
                    (base_body_tint[1] * (1.0 - hurt_flash) + 0.25 * hurt_flash)
                        * body_brightness,
                    (base_body_tint[2] * (1.0 - hurt_flash) + 0.25 * hurt_flash)
                        * body_brightness,
                    base_body_tint[3],
                ];
                let body_rot = glam::Quat::from_rotation_y(mob.yaw) * tilt;
                // Lunge offset: a brief forward shove on the
                // attack frame. The timer starts at MOB_LUNGE_TIME
                // + decays to 0; a sin curve gives a smooth
                // out-and-back motion within that window.
                let lunge_t = self
                    .mob_lunge
                    .get(&mob.id)
                    .copied()
                    .unwrap_or(0.0)
                    .clamp(0.0, MOB_LUNGE_TIME);
                let lunge_phase =
                    (1.0 - lunge_t / MOB_LUNGE_TIME).clamp(0.0, 1.0);
                let lunge_amp = 0.35
                    * (lunge_phase * std::f32::consts::PI).sin().max(0.0);
                let lunge_fwd =
                    Vec3::new(mob.yaw.cos(), 0.0, -mob.yaw.sin()) * lunge_amp;
                // Small walk-bob: id-hashed phase + day_time so
                // mobs visibly bounce while wandering. Skipped on
                // dying mobs so the death tilt stays clean.
                let walk_bob = if death_progress <= 0.0 {
                    let phase = mob.id.wrapping_mul(2654435761) as f32
                        / u32::MAX as f32
                        * std::f32::consts::TAU;
                    (self.day_time * 4.0 + phase).sin() * 0.04
                } else {
                    0.0
                };
                let body = Mat4::from_scale_rotation_translation(
                    body_scale,
                    body_rot,
                    Vec3::new(
                        mob.pos[0] + lunge_fwd.x,
                        mob.pos[1] + body_y_off - death_sink + walk_bob,
                        mob.pos[2] + lunge_fwd.z,
                    ),
                );
                renderer.draw_mesh_pbr(
                    body_use_mesh,
                    body.to_cols_array(),
                    tint,
                    0.0,
                    0.9,
                );
                let fwd = Vec3::new(mob.yaw.cos(), 0.0, -mob.yaw.sin());
                let (head_scale, head_off_y, head_off_fwd, head_mesh_use, base_head_color) =
                    if is_zombie {
                        (
                            Vec3::new(0.55, 0.55, 0.55),
                            1.85,
                            0.0,
                            player_cube_handle,
                            [0.22, 0.42, 0.22, 1.0],
                        )
                    } else if is_chicken {
                        // Small head, slight forward poke, red
                        // comb-ish tint so the chicken reads as
                        // distinct from a tiny sheep.
                        (
                            Vec3::new(0.3, 0.3, 0.3),
                            0.65,
                            0.25,
                            player_cube_handle,
                            [0.95, 0.35, 0.30, 1.0],
                        )
                    } else if is_pig {
                        (
                            Vec3::new(0.6, 0.55, 0.55),
                            0.65,
                            0.40,
                            player_cube_handle,
                            [0.95, 0.65, 0.65, 1.0],
                        )
                    } else {
                        (
                            Vec3::new(0.60, 0.45, 0.45),
                            0.8,
                            0.45,
                            head_mesh,
                            head_color,
                        )
                    };
                let head_pos = Vec3::new(
                    mob.pos[0] + lunge_fwd.x,
                    mob.pos[1] + head_off_y - death_sink + walk_bob,
                    mob.pos[2] + lunge_fwd.z,
                ) + fwd * head_off_fwd;
                let head_rot = glam::Quat::from_rotation_y(mob.yaw) * tilt;
                let head = Mat4::from_scale_rotation_translation(
                    head_scale,
                    head_rot,
                    head_pos,
                );
                let head_tint = [
                    (base_head_color[0] * (1.0 - hurt_flash) + 1.0 * hurt_flash)
                        * body_brightness,
                    (base_head_color[1] * (1.0 - hurt_flash) + 0.25 * hurt_flash)
                        * body_brightness,
                    (base_head_color[2] * (1.0 - hurt_flash) + 0.25 * hurt_flash)
                        * body_brightness,
                    base_head_color[3],
                ];
                renderer.draw_mesh_pbr(
                    head_mesh_use,
                    head.to_cols_array(),
                    head_tint,
                    0.0,
                    0.9,
                );
            }
        }
        // Render any live debris particles before the world cubes
        // so chunk faces draw on top when a particle ends up
        // inside a block.
        renderer.draw_particles(&self.particles);
        let model = Mat4::IDENTITY.to_cols_array();
        for (_coord, block, handle) in &chunk_meshes {
            if let Some(def) = registry.get(*block) {
                // Liquid voxels arrive here only when their
                // dedicated slope path is disabled (test paths or
                // unflagged builds). Generic alpha gate so any
                // F_TRANSPARENT_RENDER block reads correctly.
                let alpha = if block_is_transparent_render(*block) {
                    0.65
                } else {
                    1.0
                };
                renderer.draw_mesh_pbr(
                    *handle,
                    model,
                    [def.color[0], def.color[1], def.color[2], alpha],
                    def.metallic,
                    def.roughness,
                );
            }
        }
        // Wire outline previewing the next action. The highlight
        // mode (H key, persisted in PlayerSave) gates which
        // preview wins:
        //   Auto  -> placeable held = build preview,
        //            otherwise the break wireframe.
        //   Build -> always the build preview (uses the held
        //            block's tint if any, else a neutral cyan so
        //            the cell is still visible without a block).
        //   Break -> always the break wireframe at target.voxel.
        if let Some(hit) = target {
            let highlight = self.highlight_mode;
            let show_build = match highlight {
                HighlightMode::Auto => selected_block.is_some(),
                HighlightMode::Build => true,
                HighlightMode::Break => false,
            };
            let (cell, color) = if show_build {
                let tint = selected_block
                    .and_then(|block| registry.get(block))
                    .map(|d| {
                        [
                            (d.color[0] * 1.4).min(1.0),
                            (d.color[1] * 1.4).min(1.0),
                            (d.color[2] * 1.4).min(1.0),
                            0.95,
                        ]
                    })
                    // Cyan fallback so Build-mode without a block in
                    // hand still draws something visible. Picked to
                    // contrast with the break wireframe's black.
                    .unwrap_or([0.3, 0.85, 1.0, 0.95]);
                (hit.place_at, tint)
            } else {
                (hit.voxel, [0.0, 0.0, 0.0, 0.95])
            };
            let min = [
                cell[0] as f32 - 0.005,
                cell[1] as f32 - 0.005,
                cell[2] as f32 - 0.005,
            ];
            let max = [
                cell[0] as f32 + 1.005,
                cell[1] as f32 + 1.005,
                cell[2] as f32 + 1.005,
            ];
            renderer.debug_box_3d(min, max, color);
        }
        // Mining-progress overlay: an extra wire box tinted from
        // bright yellow (just-started) through orange to red
        // (almost broken). Drawn slightly larger than the targeted
        // voxel so it doesn't z-fight with the regular outline.
        if let Some(m) = self.mining {
            let t = (m.progress / m.total).clamp(0.0, 1.0);
            let r = 1.0;
            let g = 1.0 - t * 0.8;
            let b = 0.1 * (1.0 - t);
            let min = [
                m.voxel[0] as f32 - 0.02,
                m.voxel[1] as f32 - 0.02,
                m.voxel[2] as f32 - 0.02,
            ];
            let max = [
                m.voxel[0] as f32 + 1.02,
                m.voxel[1] as f32 + 1.02,
                m.voxel[2] as f32 + 1.02,
            ];
            renderer.debug_box_3d(min, max, [r, g, b, 1.0]);
        }
        // Chunk-grid overlay: wire boxes for the chunk the
        // player stands in + its 8 horizontal neighbours.
        // Cheap; only fires when toggled.
        if self.show_chunk_grid {
            let cs = CHUNK_SIZE as i32;
            let pc = [
                (self.player_pos.x.floor() as i32).div_euclid(cs),
                (self.player_pos.y.floor() as i32).div_euclid(cs),
                (self.player_pos.z.floor() as i32).div_euclid(cs),
            ];
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let cc = [pc[0] + dx, pc[1], pc[2] + dz];
                    let min = [
                        (cc[0] * cs) as f32,
                        (cc[1] * cs) as f32,
                        (cc[2] * cs) as f32,
                    ];
                    let max = [
                        min[0] + cs as f32,
                        min[1] + cs as f32,
                        min[2] + cs as f32,
                    ];
                    // Highlight the player's own chunk in
                    // bright yellow; neighbours dim cyan.
                    let color = if dx == 0 && dz == 0 {
                        [1.0, 0.85, 0.1, 0.95]
                    } else {
                        [0.2, 0.7, 1.0, 0.6]
                    };
                    renderer.debug_box_3d(min, max, color);
                }
            }
        }
        let render_result = if let Some(ui) = ui {
            renderer.render(|d, q, e, v, s| ui.render(d, q, e, v, s))
        } else {
            renderer.render(|_, _, _, _, _| {})
        };
        if let Err(err) = render_result {
            warn!(error = ?err, "frame render failed");
        }

        if self.requested_quit {
            std::process::exit(0);
        }
    }

    fn on_resize(&mut self, w: u32, h: u32) {
        self.viewport = (w, h);
        if let Some(r) = self.renderer.as_mut() {
            r.resize(w, h);
        }
    }
}

/// Output from one worker thread: the generated chunk + its submeshes
/// ready to upload as GPU buffers on the main thread.
struct WorkerResult {
    coord: ChunkCoord,
    chunk: Chunk,
    submeshes: Vec<ChunkSubmesh>,
}

/// One unit of work for the chunk worker pool. Carries the per-job
/// `WorldgenCtx` snapshot captured at enqueue time, so the worker
/// generates the chunk against the EXACT base seed + dim + profile
/// the player saw when the request was made. Without this, a
/// `switch_dim` between enqueue and pickup poisoned the result
/// (the worker would read fresh atomics from the wrong dim).
struct ChunkJob {
    coord: ChunkCoord,
    ctx: worldgen::WorldgenCtx,
}

/// Thread pool that runs `generate_chunk` + `chunk_mesh` off the main
/// thread. The main thread enqueues `ChunkJob` requests on
/// `req_tx`; workers produce `WorkerResult` on `res_rx`. Dropping the
/// pool closes `req_tx`; the workers see `recv` return Err and exit.
struct WorkerPool {
    req_tx: Sender<ChunkJob>,
    res_rx: Receiver<WorkerResult>,
    threads: Vec<JoinHandle<()>>,
}

impl WorkerPool {
    fn spawn(num_workers: usize) -> Self {
        let (req_tx, req_rx) = mpsc::channel::<ChunkJob>();
        let req_rx = Arc::new(Mutex::new(req_rx));
        let (res_tx, res_rx) = mpsc::channel::<WorkerResult>();
        let mut threads = Vec::with_capacity(num_workers);
        for i in 0..num_workers {
            let req_rx = Arc::clone(&req_rx);
            let res_tx = res_tx.clone();
            threads.push(
                thread::Builder::new()
                    .name(format!("voxel_worker_{i}"))
                    .spawn(move || {
                        loop {
                            // Briefly lock the shared receiver to grab the
                            // next job. The lock is held only across one
                            // recv() call; generation runs lock-free.
                            let job = {
                                let rx = req_rx.lock().expect("worker mutex");
                                match rx.recv() {
                                    Ok(j) => j,
                                    Err(_) => return,
                                }
                            };
                            // Pure-ctx call - no atomic reads. The
                            // ctx snapshot at enqueue is the
                            // source of truth for this job; a dim
                            // swap on the main thread between
                            // enqueue + pickup can no longer
                            // poison the gen.
                            let chunk = worldgen::generate_chunk_at_ctx(job.coord, &job.ctx);
                            let origin = World::chunk_world_origin(job.coord);
                            let submeshes = chunk_mesh_with_atlas(&chunk, origin, atlas_uv_rect, block_is_opaque);
                            if res_tx
                                .send(WorkerResult {
                                    coord: job.coord,
                                    chunk,
                                    submeshes,
                                })
                                .is_err()
                            {
                                return;
                            }
                        }
                    })
                    .expect("spawn voxel worker"),
            );
        }
        drop(res_tx); // workers keep their own clones
        Self {
            req_tx,
            res_rx,
            threads,
        }
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        // Close the request channel by replacing the sender with a
        // dummy that we then drop, which causes worker recv() to fail.
        // Replacing the field with a fresh channel and dropping is
        // equivalent; using `mem::replace` keeps the type stable.
        let (dummy_tx, _) = mpsc::channel::<ChunkJob>();
        let _ = std::mem::replace(&mut self.req_tx, dummy_tx);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

// `Biome`, `WORLD_SEED`, `world_seed`, `set_world_seed`,
// `seed_offset`, `biome_at`, `ore_hash`, `hash2` moved to
// `worldgen.rs`. The seed accessors stay re-exported below.
// The free-standing chunk generator the workers call
// (`generate_chunk_at`) likewise moved to `worldgen.rs`.

// `TileSpec`, `BlockType`, `BLOCK_TYPES`, `F_*` flags, atlas
// constants, and the well-known block-id constants all live in
// `block/mod.rs` now. Imported at the top of this file.

// `F_*` flag constants, `BlockType`, atlas dims and the
// `BLOCK_TYPES` table moved to `block/mod.rs`. The use clause at
// the top of this file re-imports them so existing call sites
// keep working unchanged.
// BLOCK_TYPES table + ID constants moved to block/mod.rs.


// `WorldGen` enum + `WORLDGEN` atomic moved to `worldgen.rs`.

// `block_type`, `block_has_flag`, all `block_is_*` helpers,
// `block_material`, `block_is_opaque`, `atlas_uv_rect` moved
// to `block/mod.rs`.

/// Try to load the atlas from a directory of 16x16 PNG / TGA files
/// (one per block kind). Returns `None` if the directory is missing
/// or any required texture fails to load. The names match the
/// Mojang Bedrock sample pack so users can drop those textures
/// straight in:
///
/// ```text
/// <dir>/stone.png
/// <dir>/dirt.png
/// <dir>/grass_top.png
/// <dir>/sand.png
/// <dir>/log_oak.png
/// <dir>/leaves_oak.tga    (preferred - has alpha)
/// <dir>/leaves_oak_opaque.png  (fallback)
/// ```
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
    let night = [0.04_f32, 0.06, 0.12];
    let horizon = [0.95_f32, 0.55, 0.30];
    let noon = [0.50_f32, 0.70, 0.92];
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
    let a = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
    let b = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
    let n = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    if len < 1e-6 {
        [0.0, 1.0, 0.0]
    } else {
        [n[0] / len, n[1] / len, n[2] / len]
    }
}


// `submerged_liquid_eye`, `submerged_liquid`, and
// `aabb_intersects_solid` moved to `player.rs`.

/// Move the player AABB by `delta`, resolving collision with solid
/// voxels along each axis separately. Returns the new position, plus
/// flags indicating whether the player landed on a floor (negative
/// Y resolve) or bonked their head (positive Y resolve).
/// Slab-test ray-AABB intersection. Returns the parametric `t`
/// of the entry point along `dir` (assumed approximately
/// unit-length, but any positive length works) when the ray hits
/// the box; `None` if it misses. Re-used by the mob picker.
/// Build a 1x1x1 cube mesh with per-face UVs targeting the MC
/// sheep head texture layout. The face strip lives in the top-
/// left of a 64-wide texture; each 8x8 tile maps to one face of
/// an 8x8x8 cube unwrap:
///
/// ```text
///                u
///        +---+---+---+
///        | T | B |   |     0..8: -  8..16: top  16..24: bottom
///   v    +---+---+---+---+
///        | R | F | L | K |   right, front, left, back
///        +---+---+---+---+
/// ```
///
/// `tex_w` / `tex_h` are the source texture dimensions; UVs are
/// normalised against them so the same layout works for Java
/// (64x32) and Bedrock (64x64) sheep textures.
fn build_sheep_head_mesh(tex_w: u32, tex_h: u32) -> (Vec<MeshVertex>, Vec<u32>) {
    let tw = tex_w as f32;
    let th = tex_h as f32;
    // Helper: convert pixel coordinates to normalised UV.
    let uv = |u: u32, v: u32| -> [f32; 2] { [u as f32 / tw, v as f32 / th] };
    let mut verts: Vec<MeshVertex> = Vec::with_capacity(24);
    let mut indices: Vec<u32> = Vec::with_capacity(36);
    let mut add_face = |corners: [[f32; 3]; 4],
                       normal: [f32; 3],
                       uv0: [f32; 2],
                       uv1: [f32; 2],
                       uv2: [f32; 2],
                       uv3: [f32; 2]| {
        let base = verts.len() as u32;
        let uvs = [uv0, uv1, uv2, uv3];
        for i in 0..4 {
            verts.push(MeshVertex {
                position: corners[i],
                normal,
                uv: uvs[i],
            });
        }
        // Engine cube pipeline uses CCW front-face but the
        // `cube_geometry` reference winds quads as
        // `[0, 2, 1, 0, 3, 2]` when corners are listed
        // bottom-left -> bottom-right -> top-right -> top-left
        // viewed from outside. Matching that here so back-face
        // culling hides the interior of the head, not the outside.
        indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    };
    // Cube corners centred on the origin, edge length 1. Vertex
    // order per face is bottom-left, bottom-right, top-right,
    // top-left when viewed from outside the cube. UV (0,0) is
    // top-left of the texture, (1,1) is bottom-right - so the
    // matching corner UV order is `(u0,v1)`, `(u1,v1)`, `(u1,v0)`,
    // `(u0,v0)`.
    let h = 0.5_f32;
    // Bedrock sheep.geo.json: head bone has cube size [6, 6, 8] at
    // texture origin (0, 0). That's MC box-uv layout, NOT a plain
    // 8x8 face strip. The unwrap (with `d` = depth, `w` = width,
    // `h` = height):
    //
    //   ```
    //   (d, 0)         top  size w x d  -> (8, 0)..(14, 8)
    //   (d+w, 0)       bottom size w x d -> (14, 0)..(20, 8)
    //   (0, d)         right size d x h -> (0, 8)..(8, 14)
    //   (d, d)         front size w x h -> (8, 8)..(14, 14)
    //   (d+w, d)       left  size d x h -> (14, 8)..(22, 14)
    //   (d+w+d, d)     back  size w x h -> (22, 8)..(28, 14)
    //   ```
    //
    // Our render orientation has the sheep facing +X (the head's
    // depth axis runs along world X). We map MC's "front" (face)
    // tile onto our +X face so the eyes / mouth follow the
    // sheep's movement direction.
    //
    // -Z face: head's right side (MC east). Tile (0, 8)..(8, 14).
    add_face(
        [[-h, -h, -h], [h, -h, -h], [h, h, -h], [-h, h, -h]],
        [0.0, 0.0, -1.0],
        uv(0, 14),
        uv(8, 14),
        uv(8, 8),
        uv(0, 8),
    );
    // +Z face: head's left side (MC west). Tile (14, 8)..(22, 14).
    add_face(
        [[h, -h, h], [-h, -h, h], [-h, h, h], [h, h, h]],
        [0.0, 0.0, 1.0],
        uv(14, 14),
        uv(22, 14),
        uv(22, 8),
        uv(14, 8),
    );
    // +X face: the sheep's FACE. MC "front" tile (8, 8)..(14, 14).
    add_face(
        [[h, -h, -h], [h, -h, h], [h, h, h], [h, h, -h]],
        [1.0, 0.0, 0.0],
        uv(8, 14),
        uv(14, 14),
        uv(14, 8),
        uv(8, 8),
    );
    // -X face: back of head. MC "back" tile (22, 8)..(28, 14).
    add_face(
        [[-h, -h, h], [-h, -h, -h], [-h, h, -h], [-h, h, h]],
        [-1.0, 0.0, 0.0],
        uv(22, 14),
        uv(28, 14),
        uv(28, 8),
        uv(22, 8),
    );
    // Top face. MC "top" tile (8, 0)..(14, 8).
    add_face(
        [[-h, h, -h], [h, h, -h], [h, h, h], [-h, h, h]],
        [0.0, 1.0, 0.0],
        uv(8, 8),
        uv(14, 8),
        uv(14, 0),
        uv(8, 0),
    );
    // Bottom face. MC "bottom" tile (14, 0)..(20, 8).
    add_face(
        [[-h, -h, h], [h, -h, h], [h, -h, -h], [-h, -h, -h]],
        [0.0, -1.0, 0.0],
        uv(14, 8),
        uv(20, 8),
        uv(20, 0),
        uv(14, 0),
    );
    (verts, indices)
}

/// Periodic chicken feather-lay gate (Q.3). Free fn so the test
/// harness can drive it without a full `Voxel`. Returns `true`
/// exactly on the tick the timer crosses the 12 s mark with the
/// debug toggle on AND a chicken alive on a host; the caller's
/// job is to perform the actual pickup. Resets the accumulator
/// every gated-off frame so flipping the toggle never emits a
/// stale-stored feather.
fn tick_chicken_feather_lay(
    debug_enabled: bool,
    host_role: bool,
    chicken_alive: bool,
    accum: &mut f32,
    dt: f32,
) -> bool {
    if !(debug_enabled && host_role && chicken_alive) {
        *accum = 0.0;
        return false;
    }
    *accum += dt;
    if *accum >= 12.0 {
        *accum = 0.0;
        return true;
    }
    false
}

fn ray_aabb(origin: [f32; 3], dir: [f32; 3], min: [f32; 3], max: [f32; 3]) -> Option<f32> {
    let mut tmin = 0.0_f32;
    let mut tmax = f32::INFINITY;
    for axis in 0..3 {
        let inv = 1.0 / dir[axis];
        let mut t1 = (min[axis] - origin[axis]) * inv;
        let mut t2 = (max[axis] - origin[axis]) * inv;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        tmin = tmin.max(t1);
        tmax = tmax.min(t2);
        if tmax < tmin {
            return None;
        }
    }
    if tmax < 0.0 {
        None
    } else {
        Some(tmin.max(0.0))
    }
}

fn sweep_aabb(
    world: &World,
    snow: &HashMap<[i32; 3], u8>,
    pos: Vec3,
    delta: Vec3,
) -> (Vec3, bool, bool) {
    let mut new_pos = pos;
    let mut hit_floor = false;
    let mut hit_ceiling = false;

    // X axis.
    let candidate = Vec3::new(new_pos.x + delta.x, new_pos.y, new_pos.z);
    if aabb_intersects_solid(world, snow, candidate) {
        if delta.x > 0.0 {
            let wall = (candidate.x + PLAYER_RADIUS).floor();
            new_pos.x = wall - PLAYER_RADIUS - COLLISION_EPSILON;
        } else if delta.x < 0.0 {
            let wall = (candidate.x - PLAYER_RADIUS).ceil();
            new_pos.x = wall + PLAYER_RADIUS + COLLISION_EPSILON;
        }
    } else {
        new_pos.x = candidate.x;
    }

    // Y axis.
    let candidate = Vec3::new(new_pos.x, new_pos.y + delta.y, new_pos.z);
    if aabb_intersects_solid(world, snow, candidate) {
        if delta.y > 0.0 {
            let wall = candidate.y.floor();
            new_pos.y = wall - COLLISION_EPSILON;
            hit_ceiling = true;
        } else if delta.y < 0.0 {
            let wall = (candidate.y - PLAYER_HEIGHT).ceil();
            new_pos.y = wall + PLAYER_HEIGHT + COLLISION_EPSILON;
            hit_floor = true;
        }
    } else {
        new_pos.y = candidate.y;
    }

    // Z axis.
    let candidate = Vec3::new(new_pos.x, new_pos.y, new_pos.z + delta.z);
    if aabb_intersects_solid(world, snow, candidate) {
        if delta.z > 0.0 {
            let wall = (candidate.z + PLAYER_RADIUS).floor();
            new_pos.z = wall - PLAYER_RADIUS - COLLISION_EPSILON;
        } else if delta.z < 0.0 {
            let wall = (candidate.z - PLAYER_RADIUS).ceil();
            new_pos.z = wall + PLAYER_RADIUS + COLLISION_EPSILON;
        }
    } else {
        new_pos.z = candidate.z;
    }

    (new_pos, hit_floor, hit_ceiling)
}

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
    // Scope the drag-source id through the parent ui so the same
    // SlotKind can appear in multiple layers (e.g. the hotbar Area
    // + the hotbar mirror inside the Inventory window) without
    // egui flagging it as a layer-id switch on the same widget.
    let id = ui.make_persistent_id(("inv_slot", kind));
    let resp = if draggable && slot.is_some() {
        ui.dnd_drag_source(id, kind, |ui| {
            draw_slot_widget(ui, slot, atlas, selected, egui::Sense::click_and_drag())
        })
        .response
    } else {
        draw_slot_widget(ui, slot, atlas, selected, egui::Sense::click())
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
                        WORLDGEN
                            .store(g as u8, std::sync::atomic::Ordering::Relaxed);
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

