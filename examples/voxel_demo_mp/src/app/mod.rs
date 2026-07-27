//! W47.24 - main `Voxel` runtime struct (top-level app state) +
//! the small `PluginBlock` row. Relocated from main.rs to keep
//! the bin shell small (CLAUDE.md target: < 500 LOC).
//!
//! Fields are `pub(crate)` so submodules (fire_runtime, world::*,
//! ui::render, bootstrap, etc.) can still touch them via the
//! `&mut Voxel` free-fn pattern.

#![allow(unused_imports)]

use crate::*;
use crate::lighting::HaloLighting;

// W47.28 - per-domain impl Voxel submodules.
mod chunk_rebuild;
mod chunk_runtime;
mod combat_runtime;
mod container_glue;
mod dim_runtime;
mod inventory_runtime;
mod lighting_runtime;
mod liquid_tick;
mod net_player;
mod net_session;
mod new_construct;
mod particle_runtime;
mod save_runtime;
mod tick_glue;
mod ui_glue;
mod world_init;

/// W212: per-chunk liquid meshes - `(liquid id, mesh, draw tint)` per
/// liquid kind present in the chunk. Shared by the build (liquid_tick),
/// draw (render::decor) and teardown (stream / dim reset) sites.
pub(crate) type LiquidMeshes =
    HashMap<ChunkCoord, Vec<(u16, MeshHandle, [f32; 3])>>;

pub(crate) struct Voxel {
    pub(crate) state: AppState,
    pub(crate) settings: Settings,
    pub(crate) requested_quit: bool,
    /// Wall-clock instant the app started. Subtracted each frame
    /// to produce `time_seconds` for the camera uniform so shaders
    /// (animated water) can drive procedural animation.
    pub(crate) app_start: std::time::Instant,

    pub(crate) renderer: Option<Renderer>,
    pub(crate) ui: Option<EditorUi>,
    pub(crate) registry: BlockRegistry,
    pub(crate) world: World,
    // Phase F per-dim fields. Read sites land in F.2 (multi-dim
    // save/load) + F.6 (cross-dim teleport); silence the
    // "field never read" warning so the F.1 commit is clean.
    /// Phase F: name of the currently-active dimension. Drives
    /// the chunks file path (`chunks/<current_dim>/...`), the
    /// per-dim seed (`dim_seed(base_seed, current_dim)`), the
    /// per-dim worldgen profile, and which entries in the
    /// `_by_dim` parking maps are "live" vs "parked".
    #[allow(dead_code)]
    pub(crate) current_dim: String,
    /// Base seed (matches `world.json::seed`). The effective
    /// `WORLD_SEED` atomic equals `dim_seed(base_seed, current_dim)`
    /// at any moment - rewritten on every `switch_dim` so noise
    /// callers stay deterministic per-dim.
    #[allow(dead_code)]
    pub(crate) base_seed: u32,
    /// W249: slug whose meta `enter_world` pre-applied before the
    /// initial async build. `load_world` consumes it (take) and, on
    /// match, keeps the already-built chunks instead of resetting
    /// and regenerating the whole view distance a second time.
    pub(crate) preapplied_slug: Option<String>,
    /// Parking lot for inactive-dim voxel + side-state. The
    /// active dim's slot is empty - its data lives in the
    /// top-level fields (`world`, `chests`, `furnaces`, ...).
    #[allow(dead_code)]
    pub(crate) worlds: HashMap<String, dim::DimWorld>,
    /// Parking lot for inactive-dim mobs + drops. Mirrors
    /// `worlds` - the active dim's `mobs` / `dropped_items` live
    /// in the top-level fields.
    #[allow(dead_code)]
    pub(crate) entities_by_dim: HashMap<String, dim::DimEntities>,
    /// Per-dim worldgen variant lookup (mirrors
    /// `WorldJson::dimension_worldgens` so the runtime knows
    /// which `WorldGen` to pin when switching to a dim that
    /// hasn't been visited yet this session).
    #[allow(dead_code)]
    pub(crate) dim_worldgens: HashMap<String, WorldGen>,
    /// Phase F.6: ticks the player has been stood inside a
    /// portal cell continuously. Increments while the player
    /// AABB overlaps a `PORTAL_ID` cell; resets to 0 on step
    /// out. Once it reaches `PORTAL_TRIGGER_TICKS` the runtime
    /// teleports the player to the paired dim.
    #[allow(dead_code)]
    pub(crate) portal_in_ticks: u32,
    /// Phase F.6: cool-down counter set on teleport. Decremented
    /// each tick; while > 0 the portal-overlap check is
    /// suppressed so the player doesn't immediately bounce
    /// back through the destination portal. W277: RE-ARMED to
    /// full while the player stands inside a portal (MC
    /// Entity.setAsInsidePortal), so it only runs down after
    /// stepping out of the frame.
    #[allow(dead_code)]
    pub(crate) portal_cooldown_ticks: u32,
    /// W277: 20 Hz accumulator for the portal tick (it used to run
    /// per frame, making warmup/cooldown frame-rate dependent).
    pub(crate) portal_tick_accum: f32,
    /// W281: debounce for the water/lava enter+exit one-shots. The
    /// foot/eye liquid test flips every stroke while swimming at the
    /// surface, and each flip fired splash + enter/exit cues (the
    /// user docx: spammy splashes, wedged underwater ambience).
    pub(crate) splash_sfx_cooldown: f32,
    /// Mesh handles grouped by the chunk that produced them. Lets the
    /// streaming code rebuild / unload only the chunks that change.
    ///
    /// W26.4: changed from Vec<(ChunkCoord, BlockId, MeshHandle)> to
    /// HashMap<ChunkCoord, Vec<(BlockId, MeshHandle)>> so that removal
    /// of a single chunk's meshes is O(1) instead of O(n) where n is
    /// the total number of entries across all chunks.
    pub(crate) chunk_meshes: HashMap<ChunkCoord, Vec<(BlockId, MeshHandle)>>,
    /// W321: far surface-shell tiles keyed by chunk column `[cx, cz]`
    /// (the LOD ring between `view_distance` and `lod_distance`).
    pub(crate) lod_meshes: HashMap<[i32; 2], crate::lod::LodTile>,
    /// W321: LOD tiles queued for a worker, closest-first.
    pub(crate) lod_pending: BinaryHeap<crate::lod::PendingLodTile>,
    /// W350: chunk coords whose file in THIS dim's save is known
    /// to exist (written this session, or written by the unload
    /// path once). Replaces the per-chunk `chunk_file_exists` stat
    /// in the unload filter - ~700 filesystem stats per rescan were
    /// 50-135 ms main-thread hitches at the world edge (the
    /// payload_ms in W232). A disk-loaded chunk not in this set
    /// gets ONE redundant write on its first unload, then lands
    /// here. Cleared on world/dim switch.
    pub(crate) disk_backed: std::collections::HashSet<ChunkCoord>,
    /// W353: every CHEST_ID block position in the loaded world, fed
    /// by the worker side-effect scan (gen AND disk-load paths) plus
    /// player placement. `draw_chests` renders from this set - the
    /// old source was the CONTAINER store, and a worldgen chest only
    /// mints its container lazily, so structure chests were
    /// invisible until first opened (the R17 report).
    pub(crate) chest_render_positions: std::collections::HashSet<[i32; 3]>,
    /// W321: dedup set for `lod_pending` + in-flight tiles.
    pub(crate) lod_pending_set: HashSet<[i32; 2]>,
    /// W333: FAR shells (level 2, 4x4-chunk tiles keyed by tile
    /// coordinate = chunk / 4).
    pub(crate) lod2_meshes: HashMap<[i32; 2], crate::lod::LodTile>,
    pub(crate) lod2_pending: BinaryHeap<crate::lod::PendingLodTile>,
    pub(crate) lod2_pending_set: HashSet<[i32; 2]>,
    /// W28.1: Chunks queued for generation, ordered closest-first by
    /// XZ distance from the player. Replaces VecDeque so workers
    /// always receive the most relevant chunks first.
    pub(crate) pending_chunks: BinaryHeap<PendingChunk>,
    /// Set of chunks either queued OR currently being generated by a
    /// worker thread. Used to avoid re-dispatching the same coord.
    pub(crate) pending_set: HashSet<ChunkCoord>,
    /// W359: chunks past the keep radius awaiting unload, sorted
    /// nearest-first so `pop()` takes the farthest. The boundary-cross
    /// rescan only COLLECTS them; the per-frame drain retires a
    /// bounded batch (mesh removal + save payload + world removal), so
    /// a ~700-chunk edge crossing no longer lands in one frame.
    pub(crate) pending_unload: Vec<ChunkCoord>,
    /// W359: run the side-map retain pass once the unload drain
    /// empties (the retains are box-based whole-map sweeps - one per
    /// crossing, in their own frame).
    pub(crate) unload_retain_due: bool,
    /// W360: side-map cells of the pending unload set, snapshotted
    /// once at the crossing so the per-frame drain never rescans the
    /// whole snow/liquid/torch maps (5-64 ms payload spikes, R20).
    pub(crate) pending_unload_side: crate::world_save::runtime::UnloadSideBuckets,
    /// W239: coords already handed to a worker inside a COLUMN batch
    /// job and not yet delivered. The dispatch loop batches every
    /// queued chunk of a column into one job (so the expensive
    /// per-column data computes once, on one worker); heap entries of
    /// batched-along coords still pop later and are skipped via this
    /// set. Cleared wherever `pending_set` clears.
    pub(crate) in_flight_chunks: HashSet<ChunkCoord>,
    /// W13.2: deferred neighbour-remesh queue. When a new chunk is
    /// integrated from the worker pool its 4 horizontal neighbours
    /// are added here instead of being re-meshed immediately. A
    /// bounded slice (REMESH_DEFERRED_PER_FRAME) is drained each
    /// frame so the cost is spread out rather than spiking.
    pub(crate) pending_remesh: VecDeque<ChunkCoord>,
    /// W345: LOW-priority seam cleanup. A freshly streamed chunk
    /// used to queue its 4 loaded horizontal neighbours into
    /// `pending_remesh` immediately - but the mesher emits boundary
    /// faces CONSERVATIVELY against missing neighbours (no holes
    /// ever appear), so that remesh only culls hidden faces and
    /// refreshes seam AO. During a flight it multiplied every mesh
    /// build + GPU upload ~2-3x and was a large slice of the "world
    /// loads too slowly" wall. The cleanup now waits here and only
    /// drains while the stream is idle.
    pub(crate) pending_seam_remesh: VecDeque<ChunkCoord>,
    pub(crate) pending_seam_set: HashSet<ChunkCoord>,
    /// W23.3c: mirror set for pending_remesh. Allows O(1) dedup so
    /// the same coord is never enqueued twice even when multiple
    /// arriving chunks list it as a neighbour. Cleared in sync with
    /// pending_remesh (pop removes from set; clear drains both).
    pub(crate) pending_remesh_set: HashSet<ChunkCoord>,
    /// W198.13: `(chunk_x, chunk_z, cy_min, cy_max, view_distance)` the
    /// last time the streamer ran its queue-missing + unload scans.
    /// Those two scans are O(view_dist^2 * height) HashMap lookups and
    /// O(all-loaded) respectively, yet their result only changes when
    /// the player crosses a chunk boundary (horizontal OR vertical) or
    /// the view distance changes. Gating them on this key skips ~30k
    /// pointless lookups/frame while the player stands still or merely
    /// turns in place (the residual turning judder after W198.12).
    /// Dispatch + delivery + remesh still run every frame to keep
    /// draining the existing queues.
    pub(crate) last_stream_origin: Option<(i32, i32, i32, i32, i32, i32)>,
    /// Worker pool the streaming code dispatches generation to. CPU-
    /// heavy `generate_chunk` + `chunk_mesh` run on the workers; the
    /// main thread only does the GPU buffer upload + neighbour
    /// remeshing.
    pub(crate) workers: Option<WorkerPool>,
    /// W25: monotonically-increasing edit counter per chunk coord.
    /// Incremented on every `rebuild_chunks` dispatch. Remesh results
    /// carry the revision they snapshotted; stale results (older
    /// revision than current) are discarded on delivery so a rapid
    /// block-edit sequence never overwrites a newer mesh with an
    /// older snapshot.
    pub(crate) chunk_revisions: HashMap<ChunkCoord, u64>,
    /// W27.4: number of `Job::Lighting` dispatches still in flight.
    /// Incremented on dispatch, decremented on `WorkerResult::Lit`
    /// delivery. The loading phase waits for this to reach 0 before
    /// transitioning from `BuildingLighting` to `BuildingMeshes`.
    pub(crate) pending_lighting_jobs: u32,
    /// W284: consecutive `tick_lighting` calls spent AT the in-flight
    /// cap without a slot freeing up. Feeds the wedged-counter
    /// self-heal (a leaked decrement otherwise disables lighting for
    /// the rest of the session).
    pub(crate) lighting_capped_frames: u32,
    /// Most recent ray hit while looking around in Play. Drives the
    /// crosshair / wireframe overlay and the RMB-place step. Skips
    /// `F_RAY_PASS` decorations (torch, flowers) so the player can
    /// place a block adjacent to a torch without targeting it.
    pub(crate) target: Option<RayHit>,
    /// Parallel ray that stops on EVERY non-air voxel, decorations
    /// included. Drives mining (LMB) so the torch / flower itself
    /// is breakable when the crosshair sits on it - the regular
    /// `target` ray would tunnel through and pick the block behind.
    pub(crate) mine_target: Option<RayHit>,

    pub(crate) player_pos: Vec3,
    pub(crate) player_vel: Vec3,
    pub(crate) player_yaw: f32,
    pub(crate) player_pitch: f32,

    // True while the OS reports this window as focused. Cursor
    // lock + mouse-motion handling are gated on it so two instances
    // on the same machine don't fight over input.
    pub(crate) has_focus: bool,
    pub(crate) last_mouse: Option<[f32; 2]>,
    pub(crate) mouse_dx: f32,
    pub(crate) mouse_dy: f32,
    pub(crate) esc_prev: bool,
    /// Previous-frame state of the E key. Drives the inventory-open
    /// toggle: only fires on the rising edge so holding E doesn't
    /// flicker the overlay.
    pub(crate) e_prev: bool,
    /// Previous-frame state of the F key. Rising edge consumes
    /// one piece of food from the inventory + restores HP.
    pub(crate) f_prev: bool,
    /// W370: active eating session - (food block id, seconds
    /// elapsed). MC use-duration 1.6 s; releasing F cancels.
    pub(crate) eating: Option<(u16, f32)>,
    /// W10.3: previous-frame state of the Space key. Rising-edge
    /// detection is the cheap way to count discrete press events
    /// for the double-tap fly toggle (the jump path already uses
    /// `is_pressed` for hold-to-jump, so we can't piggy-back).
    pub(crate) space_prev: bool,
    /// W10.3: seconds since the last Space rising edge. A second
    /// rising edge inside `DOUBLE_TAP_WINDOW_SECS` toggles
    /// `abilities.flying` when `abilities.mayfly` is true. Mirrors
    /// MC's LocalPlayer.java tick handler. `None` = no recent tap
    /// is being tracked.
    pub(crate) last_space_press: Option<f32>,
    /// F1 toggles HUD visibility. Useful for clean screenshots
    /// and stress-testing the renderer without overlay overhead.
    pub(crate) hud_visible: bool,
    pub(crate) f1_prev: bool,
    /// True while the chat input box is open. Gates player input
    /// like the inventory / MP menu overlays.
    pub(crate) chat_open: bool,
    /// Buffer the chat input box edits while open.
    pub(crate) chat_input: String,
    /// Recent chat lines: `(arrival_instant, formatted_text)`.
    /// Older entries fade out + drop after a few seconds.
    pub(crate) chat_log: Vec<(std::time::Instant, String)>,
    /// Player's previously sent chat lines (most recent last).
    /// Used by Up/Down arrow scrolling in the chat input box.
    /// Capped at 20 entries so very long sessions don't grow it
    /// unbounded.
    pub(crate) chat_history: Vec<String>,
    /// Index of the currently-shown history entry, or `None` when
    /// the player is typing fresh text. Reset whenever a new line
    /// is sent.
    pub(crate) chat_history_idx: Option<usize>,
    /// W11.5: tab-autocomplete popup state. `None` when the
    /// popup is hidden (no candidates / chat closed). `Some` when
    /// the popup is visible above the chat input box. The
    /// `selected_index` cycles with Tab + arrow keys.
    pub(crate) chat_completion: Option<ChatCompletionPopup>,
    /// Previous-frame state of the T key for rising-edge open.
    pub(crate) t_prev: bool,
    pub(crate) lmb_pending: bool,
    pub(crate) rmb_pending: bool,
    /// Level state of the mouse buttons (set on Pressed events,
    /// cleared on Released). Drives the hold-to-mine loop - the
    /// edge-triggered `lmb_pending` still routes single clicks to
    /// melee attacks on mobs.
    pub(crate) lmb_held: bool,
    pub(crate) rmb_held: bool,
    /// Rising-edge of the LMB this frame. Set true on the frame the
    /// button transitions from up to down, cleared at end of tick.
    /// Used by the creative insta-break path so each click destroys
    /// exactly one block instead of chaining across the 2-3 frames
    /// `lmb_held` is observed true per physical click.
    pub(crate) lmb_just_pressed: bool,
    /// Active mining session. `None` when the player isn't holding
    /// LMB on a voxel; populated with the targeted cell + the time
    /// elapsed / required to break the block. Cancels when LMB
    /// releases, the aim moves to a different voxel, or the
    /// targeted voxel disappears (someone else broke it first).
    pub(crate) mining: Option<MiningState>,
    /// Current player hit points. Capped at `PLAYER_MAX_HEALTH`.
    /// Reaches zero = death, which respawns at origin + wipes the
    /// storage rows the same way the fall-out path already does.
    pub(crate) player_health: f32,
    /// World-Y the player last left solid ground at. `None` when
    /// grounded; populated on the rising edge of an airborne
    /// transition + read on landing for fall-damage calculation.
    pub(crate) fall_start_y: Option<f32>,
    /// Seconds remaining in the red screen-flash overlay that
    /// fires on any non-trivial damage event. Decays in update.
    pub(crate) hurt_flash: f32,
    /// Seconds elapsed since the last damage. Once it crosses
    /// `REGEN_DELAY`, HP starts ticking up at `REGEN_RATE` HP/s
    /// (capped at `PLAYER_MAX_HEALTH`).
    pub(crate) regen_accum: f32,
    /// Mobs the local player has killed this session. Bumped in
    /// `damage_mob` whenever the killing blow is attributed to the
    /// host (id 0). Surfaced in the HUD + F3 debug panel.
    pub(crate) kill_count: u32,
    /// Session counters for break / place actions the local
    /// player completed. Surfaced via /stats.
    pub(crate) blocks_broken: u64,
    pub(crate) blocks_placed: u64,
    /// Cumulative horizontal travel distance for /stats reports.
    /// Accumulates each tick whenever the player moves on the
    /// X/Z plane and resets at session boundaries (no persist).
    pub(crate) distance_walked: f32,
    /// Last frame's XZ position used for the delta calculation.
    pub(crate) last_walk_xz: Option<(f32, f32)>,
    /// Recent local placement history (oldest first). Drives the
    /// Z-key "undo last place" affordance. Capped to a small
    /// window so a long session doesn't bloat memory.
    pub(crate) place_history: Vec<[i32; 3]>,
    pub(crate) z_prev: bool,
    /// G toggles a wireframe overlay of the chunks immediately
    /// around the player. Useful for verifying streaming bounds
    /// + chunk loading patterns.
    pub(crate) show_chunk_grid: bool,
    pub(crate) g_prev: bool,
    /// Targeting wireframe mode. H cycles
    /// `Auto -> Build -> Break -> Auto`. Persisted per-player in
    /// `PlayerSave::highlight_mode` so the choice survives across
    /// sessions.
    pub(crate) highlight_mode: HighlightMode,
    /// Edge-detect helper for the H toggle: avoids re-triggering
    /// every frame the key is physically held.
    pub(crate) h_prev: bool,
    /// Current camera vertical FOV in radians. Smoothly lerps
    /// toward a wider angle while the player sprints, snaps back
    /// when they stop. Read by both the wgpu camera + the egui
    /// HUD's screen-projection so the two stay in sync.
    pub(crate) fov_y_rad: f32,
    /// W415: the MC hunger state (food / saturation / exhaustion).
    pub(crate) player_food: crate::hunger::FoodData,
    /// W417: seconds the bow has been drawn; None = not drawing.
    pub(crate) bow_charge: Option<f32>,
    /// W418: running experience-point total (levels derive via
    /// `xp::level_progress`).
    pub(crate) player_xp: u32,
    /// W419: worn armor `[helmet, chestplate, leggings, boots]`.
    pub(crate) armor_slots: [Option<(u16, u8)>; 4],
    /// `true` for the last frame's "are we actually sprinting"
    /// state - used by the FOV lerp.
    pub(crate) sprinting: bool,
    /// Breath remaining in seconds (0..=AIR_MAX_SEC). Drains
    /// while submerged in a `drains_air` liquid; refills 4x as
    /// fast on the surface. Hitting zero triggers drowning
    /// damage at `DROWN_DAMAGE_PER_SEC`.
    pub(crate) player_air: f32,
    /// Sprint stamina in 0..STAMINA_MAX. Drains while sprinting,
    /// regens when not. Reaching zero blocks the sprint speed
    /// bonus until it regens above the small reactivation
    /// threshold.
    pub(crate) stamina: f32,
    /// Per-remote-player interpolation buffer. We keep the
    /// previous + current `PlayerMsg` and their arrival instants
    /// so the render path can lerp between them at `now - delay`,
    /// turning a choppy pose stream (one update per net tick) into
    /// smooth motion regardless of the send rate. Both Server and
    /// Client roles write into this map - the host needs the same
    /// smoothing for the joined players it renders locally.
    pub(crate) remote_interp: HashMap<u32, RemoteInterp>,
    /// Local player's display name. Sent to the server as the
    /// `Hello` name (so the server's `client_name` lookup returns
    /// it), used as the chat sender tag for the host, and pushed
    /// to other clients via `vx.names`. Defaults to a random
    /// `Player####` if the user hasn't set one in settings.
    pub(crate) player_name: String,
    /// Latest known display name per player id. Filled from the
    /// `vx.names` roster the server broadcasts on join/leave. Used
    /// for nameplates above remote players and the Tab playerlist
    /// overlay. The local player's id always maps to
    /// `self.player_name`; remote ids come from the server.
    pub(crate) player_names: HashMap<u32, String>,
    /// True while Tab is held. Drives the player list overlay
    /// shown in the HUD.
    pub(crate) tab_held: bool,
    /// Floating "+N <block>" toasts shown on the right side of
    /// the HUD whenever the player picks something up. Entries
    /// fade out after a couple of seconds; consecutive picks of
    /// the same block collapse into a single counter.
    pub(crate) pickup_toasts: Vec<(std::time::Instant, BlockId, u8)>,
    /// Flat slot array. Indices `0..HOTBAR_SLOTS` are the hotbar
    /// row (visible at the bottom of the screen, scrollable via
    /// mouse wheel); indices `HOTBAR_SLOTS..INVENTORY_SLOTS` are
    /// the extra rows shown when the player opens the inventory
    /// via `E`. Each slot holds an optional `(BlockId, count)` -
    /// `count` saturates at `STACK_MAX`.
    pub(crate) inventory: [Option<(BlockId, u8)>; INVENTORY_SLOTS],
    /// W5.2b: per-slot durability map. Currently only flint+steel
    /// (id 48) ever lands here; other items have infinite uses.
    /// Keyed by inventory slot index. When a slot moves (drag&drop
    /// in the UI), the inventory module's mutation path updates
    /// the map via `move_durability_on_drag` so the value follows
    /// the stack. Slots that empty clear their entry; fresh
    /// pickups of flint+steel mint `FLINT_AND_STEEL_MAX_USES`.
    ///
    /// Chosen over extending the slot's tuple to `(BlockId, u8,
    /// Option<u16>)` to keep the on-disk player save format
    /// unchanged for Wave 5; a later commit can fold this into
    /// the slot tuple once the durability concept stabilises and
    /// applies to more items (axes, swords, etc).
    pub(crate) item_durability: HashMap<usize, u16>,
    pub(crate) selected_slot: usize,
    /// True when the inventory overlay is up. Pauses player input
    /// + shows the cursor.
    pub(crate) inventory_open: bool,
    /// In multiplayer roles, Esc opens an overlay menu *without*
    /// pausing the world simulation - other peers shouldn't see a
    /// frozen ghost just because someone hit Esc. This flag drives
    /// the overlay; `update_play` keeps ticking either way.
    pub(crate) mp_menu_open: bool,
    /// 2x2 crafting grid laid out row-major (idx = row*2 + col).
    /// Drained on a successful craft. Only relevant while the
    /// inventory overlay is open.
    pub(crate) craft_grid: [Option<(BlockId, u8)>; 4],
    /// Accumulated mouse-wheel ticks since the last tick consume; the
    /// sign drives slot selection.
    pub(crate) wheel_accum: f32,
    /// `true` when the player's feet sit on a solid voxel; resets
    /// every frame after collision resolution.
    pub(crate) grounded: bool,

    pub(crate) viewport: (u32, u32),
    pub(crate) fps_ema: f32,
    /// Accumulated in-world time (seconds). Wraps every
    /// `DAY_LENGTH_S`. Drives sun direction + sky color so the
    /// world has a visible day/night cycle.
    pub(crate) day_time: f32,

    /// Networking role + state. `Offline` runs the demo unchanged;
    /// `Server` / `Client` flip on the per-tick replication path.
    pub(crate) net: NetRole,
    /// Authoritative edit log on the server. Late-joining clients
    /// get the full history rebroadcast so their generated world
    /// catches up with everyone else's. Server-only; clients leave
    /// it empty.
    pub(crate) edit_history: Vec<EditMsg>,
    /// Client ids the server saw last tick. Used to detect joins
    /// for the catch-up broadcast.
    pub(crate) known_client_ids: HashSet<u32>,
    /// Last time the server's `edit_history` was flushed to disk.
    /// `None` until the first save fires (30 s after Play start);
    /// any later instant gates the next periodic flush.
    pub(crate) last_state_save: Option<std::time::Instant>,
    /// Side map from client name -> their persisted inventory.
    /// Populated on `load_server_state` for hosts; consulted when
    /// a client connects so reconnects pick up the same stacks.
    /// Empty in offline / client modes.
    pub(crate) saved_inventories: HashMap<String, ClientInv>,
    /// Slug of the world this session is bound to. Drives the
    /// `worlds/<slug>/` directory that `world_save::save_world` /
    /// `world_save::load_world` use. Defaulted to `Some("legacy")`
    /// for the cold-start path: the legacy-migration step in
    /// `load_server_state` overrides it if a pre-A4 single-file save
    /// is present, and the Worlds picker (Phase G scaffolding in B.7a)
    /// rewrites it on a New / Load click.
    pub(crate) current_world_slug: Option<String>,
    /// W8.2: current player game mode. Loaded from the player file
    /// on join + persisted on every save. Defaults to Survival on
    /// a fresh runtime.
    ///
    /// Wave 10 wired the runtime checks: `fly_mode` / `god_mode` /
    /// insta-break / creative tab now derive from `abilities`
    /// below, which is recomputed every time this field changes.
    pub(crate) game_mode: game_mode::GameMode,
    /// W10.1: cached `Abilities` bundle for `game_mode`. Updated
    /// in lockstep with `game_mode` (world load, `/gamemode`
    /// command). Consumers branch on individual flags here
    /// instead of re-deriving them from the enum so future modes
    /// (Adventure, Spectator) can opt in without touching every
    /// call site. The `flying` flag is the toggleable live state
    /// the double-tap-Space detector mutates - other flags are
    /// pure functions of `game_mode`.
    pub(crate) abilities: game_mode::Abilities,
    /// W10.6: Creative-mode searchable inventory tab state.
    /// Separate from `inventory_open` (the survival inventory)
    /// so opening one closes the other. The tab is gated on
    /// `abilities.mayfly = true` indirectly via the E-key
    /// handler in `update_play`.
    pub(crate) creative_tab: creative_tab::CreativeTabState,
    /// W20.4: live search query for the recipe book. Lowercased
    /// substring match against recipe output display name.
    pub(crate) recipe_book_query: String,
    /// W8.1: gate for the world simulation tick. Set to `true` only
    /// after the user explicitly enters a world via
    /// PlayOffline/NewWorld+enter/DirectConnect/JoinServer; the bin
    /// boots with the menu over an empty Voxel so no chunks / mobs /
    /// liquids / lighting workers run in the background. Returning
    /// to the top-level main menu (action_main_menu /
    /// action_mp_disconnect) tears the world down + flips this back
    /// to `false`.
    pub(crate) world_loaded: bool,
    /// Monotonic tick counter incremented at the start of each
    /// `tick_net` call. Used to throttle pending-edit / pending-
    /// inventory-action resends so we don't spam UDP every frame.
    pub(crate) net_tick: u64,
    /// Audio mixer. Lazily initialised on first `on_start` because
    /// the cpal backend can fail (no output device, locked driver,
    /// etc.) and we don't want the demo to refuse to launch.
    pub(crate) audio: Option<infengine_audio::AudioSystem>,
    /// Multi-variant sound bank keyed by event name (e.g.
    /// `"dig:grass"`, `"step:wood"`, `"mob/sheep/say"`). Loaded
    /// from `--sounds <dir>` on startup; missing keys fall back
    /// to the procedural synth in `sfx_at`.
    pub(crate) audio_bank: infengine_audio::AudioBank,
    /// Optional directory of .ogg sound effects. When set, `on_start`
    /// tries to load `break.ogg`, `place.ogg`, `step.ogg` into
    /// `sound_assets`.
    pub(crate) sounds_dir: Option<std::path::PathBuf>,
    /// Footstep cooldown: counts down each frame while the player
    /// is moving + grounded. Resets to STEP_PERIOD whenever a
    /// step sting fires.
    pub(crate) step_cooldown: f32,
    /// W123: rate-limit stamp for the item-pickup pop so a mass
    /// vacuum pickup doesn't machine-gun `entity.item.pickup`.
    pub(crate) last_pickup_sfx: Option<std::time::Instant>,
    /// W123: seconds until the next `weather.rain` ambience segment
    /// fires while weather == rain. Transient (not saved).
    pub(crate) rain_sfx_accum: f32,
    /// W123: previous-frame submerged flag; drives the one-shot
    /// `ambient.underwater.enter` / `.exit` stings.
    pub(crate) underwater_was: bool,
    /// W123: seconds until the next `ambient.underwater.loop`
    /// segment refires while the camera stays submerged.
    pub(crate) underwater_sfx_accum: f32,
    /// W354: the running underwater ambience loop; stopped (with a
    /// fade) the moment the eye surfaces. The old periodic one-shot
    /// kept ringing for the rest of the long sample after surfacing.
    pub(crate) underwater_loop: Option<infengine_audio::ActiveSound>,
    /// W123: seconds until the next eligible `ambient.cave` sting
    /// (rolls only while the player is in a dark underground spot).
    pub(crate) cave_sfx_in: f32,
    /// W123: seconds until the next background-music track. The
    /// track itself lazy-loads into the bank under the "music" key
    /// (one track resident at a time).
    pub(crate) music_in: f32,
    /// W232: in-flight background music decode. A full track decode
    /// takes hundreds of ms (the 295 ms world-tick hitch), so
    /// `play_next_music_track` decodes on a named thread and
    /// `audio_ambience::tick` drains the result here (bank insert +
    /// play, both cheap). `Some` while a decode is in flight - also
    /// the "don't start another" gate.
    pub(crate) music_decode_rx:
        Option<std::sync::mpsc::Receiver<Option<infengine_audio::Sound>>>,
    /// W232: accumulator gating the random-tick pass to MC's 20 Hz
    /// game-tick rate (the W104/W198.7 pattern; it used to run per
    /// render frame).
    pub(crate) random_tick_accum: f32,
    /// W236: true once the Finalising step of the loading state
    /// machine has applied persisted server state (load_server_state)
    /// for the world being entered. Lets the BuildingMeshes phase
    /// re-fire after the load enqueues its worker remeshes without
    /// running the load twice. Reset by `build_world`.
    pub(crate) load_state_applied: bool,
    /// W237: lighting segments awaiting worker dispatch. Snapshotting
    /// a segment clones its chunks on the main thread, so
    /// `drain_lighting_segments` spends at most a small time budget
    /// per tick and the rest waits here.
    pub(crate) pending_lighting_segments:
        std::collections::VecDeque<(i32, i32)>,
    /// W334: per queued 2x2-column key, the union of chunks awaiting
    /// a lighting flood. Streaming delivers a column's chunks across
    /// many ticks; merging repeat marks into the queued segment stops
    /// the same area being re-flooded once per delivery (a large part
    /// of the pinned light_jobs=24 saturation).
    pub(crate) pending_lighting_chunks:
        std::collections::HashMap<(i32, i32), Vec<ChunkCoord>>,
    /// Seconds since the last heartbeat thump. Drives the low-HP
    /// audio cue: fires more rapidly the closer the player is to
    /// dying. Stays at 0 above the threshold.
    pub(crate) heartbeat_accum: f32,
    /// Cactus contact damage interval timer.
    pub(crate) cactus_damage_accum: f32,
    /// Advancing phase used by the camera head-bob. Increments
    /// each frame proportionally to the player's horizontal speed
    /// when grounded; eases back toward zero off-ground. Kept
    /// separate from `day_time` so a stationary player has a
    /// stable view.
    pub(crate) head_bob_phase: f32,
    /// 0 = first person, 1 = third person behind, 2 = third
    /// person front. Cycled with F5 the way Minecraft does it.
    pub(crate) camera_mode: u8,
    pub(crate) f5_prev: bool,
    /// F6 toggles a runtime debug panel with sliders for time of
    /// day, weather, FOV, render distance, etc.
    pub(crate) show_debug_panel: bool,
    pub(crate) f6_prev: bool,
    /// Debug toggle for liquid spread bias-toward-drops
    /// pathfinding. When false, spread is purely radial (every
    /// air neighbour gets level+1 regardless of downhill
    /// distance). Useful for visualising the raw cellular
    /// automaton without the cliff-streaming heuristic.
    pub(crate) liquid_bias_drops: bool,
    /// F4 cycles weather modes; mirrors /weather. Host
    /// broadcasts the change so peers stay in sync.
    pub(crate) f4_prev: bool,
    pub(crate) q_prev: bool,
    /// Player's saved home position. /sethome stores the current
    /// pose; /home teleports back. Persisted via the config file
    /// so the location survives across launches.
    pub(crate) home_pos: Option<[f32; 3]>,
    /// Custom respawn point. None falls back to the origin
    /// pad. Persisted to the config file like home_pos.
    pub(crate) spawn_pos: Option<[f32; 3]>,
    /// Wallclock instant the session started running. Used by the
    /// F3 debug overlay to show how long the player has been
    /// playing. Stays None until on_start runs.
    pub(crate) session_started: Option<std::time::Instant>,
    /// Last time the server broadcast its authoritative
    /// `day_time`. Used to throttle the broadcast cadence to
    /// roughly once per second.
    pub(crate) last_day_broadcast: Option<std::time::Instant>,
    /// Creative-style fly mode. Toggled by `/fly`. Disables
    /// gravity + fall damage and remaps Space / Shift to vertical
    /// thrust. Off by default so a vanilla session is unchanged.
    pub(crate) fly_mode: bool,
    /// `/godmode` toggle. When on, apply_player_damage is a
    /// no-op so the local player can't be hurt. Useful for
    /// testing the world without dying.
    pub(crate) god_mode: bool,
    /// Brief post-respawn invincibility timer in seconds.
    /// `apply_player_damage` swallows hits while > 0 so the
    /// player can recover after a death drop without dying again
    /// instantly to a lingering mob.
    pub(crate) invuln_remaining: f32,
    /// Tracks the previous tick's is_night state so the host can
    /// emit chat lines on the day <-> night transitions without
    /// spamming every frame.
    pub(crate) was_night: bool,
    /// Tracks the previous tick's submerged state so we can
    /// fire a splash sting + droplet particles on the air ->
    /// water transition (and a quieter exit cue going out).
    pub(crate) was_in_water: bool,
    /// `/lock` toggle on the host. When on, client-issued
    /// `vx.edit` packets are dropped so only the host can modify
    /// the world. Useful for showcase/no-grief sessions.
    pub(crate) world_locked: bool,
    /// Maximum concurrent live mob count. Tweakable via /mobcap
    /// so players on slow machines can cut the spawn pressure;
    /// defaults to `MOB_CAP_DEFAULT`.
    pub(crate) mob_cap: usize,
    /// `/freezetime` toggle. When on, the day_time clock stops
    /// advancing so the sun + sky stay frozen. Useful for
    /// screenshots / debugging.
    pub(crate) freeze_time: bool,
    /// 0 = clear, 1 = rain. Drives per-frame rain particle
    /// spawning above the player so the demo can have weather
    /// without actually wiring it into the gameplay.
    pub(crate) weather: u8,
    /// Names the player has muted via /mute. Chat lines whose
    /// sender tag matches one of these (case-insensitive) are
    /// dropped before they hit `chat_log`. Stored by lowercased
    /// name to keep matching trivial.
    pub(crate) muted_players: HashSet<String>,
    /// Seconds remaining in the current lightning flash. While
    /// positive the sky washes toward white. Decays each frame
    /// via the update tick.
    pub(crate) lightning_flash: f32,
    /// Accumulator that fires periodic snow-layer placement
    /// while weather=2 (host-only). Drives the visual snow
    /// blanket that grows on top of nearby grass voxels.
    pub(crate) snow_accum: f32,
    /// Seconds between snow paint events while weather=2. Lower
    /// = denser snowfall. Adjustable from the F6 debug panel.
    pub(crate) snow_rate: f32,
    /// Accumulator for the optional chicken feather-laying debug
    /// loop. While `debug_chicken_feathers` is on, hosts drop one
    /// feather into the host's inventory every ~12 s as long as a
    /// live chicken exists. Off by default - see the Q.3 fix
    /// (feathers were spawning continuously in inventory mid-air
    /// before this got gated). Toggle via `/debug feathers on|off`.
    pub(crate) chicken_lay_accum: f32,
    /// Real-time accumulator that turns the variable-dt frame
    /// loop into 20 Hz logical drop ticks. Drained inside
    /// `drops::tick_physics` - every `TICK_DT_SECS` (0.05 s) of
    /// real time it fires one `drops::step_logical_tick`. Lives
    /// here because the loop may straddle multiple ticks during a
    /// slow frame and the accumulator has to persist between
    /// frames.
    pub(crate) drop_tick_accum: f32,
    /// Gate for the periodic chicken feather lay loop. Defaults to
    /// `false`; flip on with `/debug feathers on` for testing.
    pub(crate) debug_chicken_feathers: bool,
    /// Seconds until the next lightning strike attempt while
    /// weather=1. Initialised to a positive value so the first
    /// strike isn't instant on /weather rain.
    pub(crate) next_lightning_at: f32,
    /// F3 toggle: show a debug-info panel with player pos, chunk
    /// counts, net status, etc. Default off so the casual HUD
    /// stays clean.
    pub(crate) show_debug: bool,
    pub(crate) f3_prev: bool,
    /// World coords of every placed torch. Updated on every world
    /// edit (break clears, place inserts). Used per frame to push
    /// point lights to the renderer near the player.
    pub(crate) torch_positions: HashSet<[i32; 3]>,
    /// W12.5: World coords + emitter block-id of every loaded MC
    /// light source (torch, glowstone, lava, fire, soul_torch,
    /// sea_lantern, jack_o_lantern, ... - any block with
    /// `light_emission_for_id(id) > 0`). Mirrors `torch_positions`
    /// for the broader MC light-source set so the per-frame
    /// halo picker can choose 8 nearest emitters of any kind, not
    /// just torches + glowstone. Maintained by `world_set_tracked`
    /// alongside `torch_positions`.
    pub(crate) emitter_positions: HashMap<[i32; 3], u16>,
    /// W302: template-entity anchors already spawned (persistent -
    /// saves in entities v3). Chunk regen checks before minting.
    pub(crate) structure_mob_anchors: HashSet<[i32; 3]>,
    /// W5.2a: tracker for placed portal blocks. Driven by
    /// `world_set_tracked` (the central edit path) so the renderer
    /// can walk a small per-frame set instead of scanning every
    /// chunk for `PORTAL_ID`. Lit portals don't move; the set
    /// stays empty until `try_ignite` fills in interior cells.
    pub(crate) portal_positions: HashSet<[i32; 3]>,
    /// W242.2: world coords of every loaded monster-spawner block.
    /// Fed by the worker side-effect scan (generated + disk-streamed
    /// chunks), the save-load chunk scan, and `world_set_tracked`
    /// (place/break); pruned by the unload retain pass. The spawner
    /// tick + the ambient cage particles walk this set instead of
    /// scanning chunks.
    pub(crate) spawner_positions: HashSet<[i32; 3]>,
    /// W242.2: per-spawner runtime tick state (delay countdown +
    /// RNG stream), minted lazily when a player first comes within
    /// activation range. NOT persisted - a reload re-inits to the
    /// MC initial 20-tick delay (see `mob::spawner` module docs).
    pub(crate) spawner_states: HashMap<[i32; 3], crate::mob::spawner::SpawnerState>,
    /// W250: worker-resolved entity kind per spawner (the mineshaft
    /// cave-spider stand-in probe needs worldgen caches only the
    /// workers keep warm). Missing entries fall back to the pure
    /// dungeon-table function - never to a main-thread membership
    /// probe.
    pub(crate) spawner_kinds: HashMap<[i32; 3], u8>,
    /// W250: cage display-mob spin angle per spawner (MC
    /// `BaseSpawner.spin`, in MC spin units - the render multiplies
    /// by 10 deg). Pure visual: not persisted, not parked per-dim,
    /// re-inits at 0 like a freshly loaded MC block entity.
    pub(crate) spawner_spin: HashMap<[i32; 3], f32>,
    /// W279: stair orientation per voxel (facing bits 0-1 in the
    /// door convention + TRAPDOOR_TOP_BIT for upside-down halves),
    /// recorded from template placements; stairs without an entry
    /// mesh the static default model (player-placed).
    pub(crate) stair_states: HashMap<[i32; 3], u8>,
    /// Sticky torch orientations keyed by voxel. 0 = floor,
    /// 1 = wall pointing -X (mounted on +X face of a block at
    /// voxel.x+1), 2 = +X, 3 = -Z, 4 = +Z. Set at placement
    /// time from the raycast normal so neighbouring builds
    /// don't reorient an existing torch.
    pub(crate) torch_orient: HashMap<[i32; 3], u8>,
    /// Voxels currently holding a thin snow layer + the layer
    /// count (1..=8). MC-style stacking: each fall raises an
    /// existing voxel's layer until it tops out, then a fresh
    /// snow voxel is painted on top.
    pub(crate) snow_positions: HashMap<[i32; 3], u8>,
    /// Per-chest container state, keyed by absolute world voxel
    /// coords. An entry is inserted when the player places a
    /// chest block + removed when the chest is broken (after the
    /// contents drop as item entities). The container itself is
    /// a 27-slot `Container` (see `block::chest::CHEST_SLOTS`).
    /// Persisted via `world_save/containers.rs`.
    pub(crate) chests: HashMap<[i32; 3], container::Container>,
    /// W126: per-chest facing (0=+X,1=+Z,2=-X,3=-Z = latch
    /// direction), set at place time (toward the placer). Drives
    /// the entity-render yaw + double-chest pairing.
    pub(crate) chest_facing: HashMap<[i32; 3], u8>,
    /// W126: lid animation state per chest (0 closed .. 1 open).
    /// Transient; advanced by the render pass.
    pub(crate) chest_lid_anim: HashMap<[i32; 3], f32>,
    /// W126: baked chest entity meshes (normal + double halves).
    pub(crate) chest_meshes: crate::render::chests::ChestMeshes,
    /// W366: baked bed head/foot models per dye color.
    pub(crate) bed_meshes: crate::render::beds::BedMeshes,
    /// W400: bell block entity - baked body mesh + cell index
    /// (worker gen side effect + the emitter rescan on load).
    pub(crate) bell_mesh: Option<crate::entity_model::BakedModel>,
    pub(crate) bell_positions: std::collections::HashSet<[i32; 3]>,
    /// W369: crack overlay for the active mining session.
    pub(crate) break_overlay: crate::render::break_overlay::BreakOverlay,
    /// W366: bed piece states - facing bits 0-1 (foot -> head
    /// direction), bit 2 = head half. Fed by template carve (kind 6)
    /// and player placement; cells without a row fall back to the
    /// legacy wool cube in the chunk mesh.
    pub(crate) bed_states: HashMap<[i32; 3], u8>,
    /// Position of the chest the player has open. `None` means no
    /// chest UI is visible. Set by an RMB-on-chest interaction +
    /// cleared by ESC / walking too far / opening another panel.
    pub(crate) chest_open: Option<[i32; 3]>,
    /// W5.3a: server-side viewer registration. Maps chest pos to
    /// the client id currently looking at it. Single-viewer for
    /// Wave 5; opening from a new client replaces the entry. Used
    /// by `apply_chest_action_*` paths to fan out `ChestStateMsg`
    /// updates only to the active viewer. Empty on clients.
    pub(crate) chest_viewers: HashMap<[i32; 3], u32>,
    /// W5.3b: mirror of `chest_viewers` for furnaces. Same single-
    /// viewer contract.
    pub(crate) furnace_viewers: HashMap<[i32; 3], u32>,
    /// W5.3b: per-viewer throttle timestamps for furnace state
    /// snapshots. Keyed by chest/furnace pos. Furnaces tick fast
    /// (cook progress changes every frame), so we cap broadcast
    /// frequency at ~5 Hz while a viewer is registered.
    pub(crate) furnace_state_last_sent_ms: HashMap<[i32; 3], u64>,
    /// Cursor-held stack while the chest UI is open. Mirrors how
    /// the standard inventory UI tracks a half / full stack drag;
    /// here it's a simple `Option<(item_id, count)>` because the
    /// chest panel uses the generic `container::ui::draw_grid`
    /// click-and-drop flow rather than the legacy DnD widget.
    pub(crate) chest_held: Option<(u16, u8)>,
    /// Per-furnace 3-slot container state (input / fuel / output),
    /// keyed by absolute world voxel coords. Parallel to `chests`.
    /// Entries are inserted on place + removed on break (after the
    /// drops fall as item entities). The runtime cook progress
    /// lives in the sibling `furnace_runtimes` map below; splitting
    /// the slot vec from the live timers keeps the save schema
    /// tight (the on-disk shape persists both side by side under
    /// `containers/<dim>.bin` v2).
    pub(crate) furnaces: HashMap<[i32; 3], container::Container>,
    /// Per-cell fire age (0..=15), keyed by absolute world voxel
    /// coords. Q.4b: parallel to `chests` / `furnaces`. The block
    /// at the keyed cell is `FIRE_ID`; the age value drives the
    /// extinguish + spread random-tick algorithm (MC
    /// FireBlock.java::tick).
    pub(crate) fires: HashMap<[i32; 3], u8>,
    /// Game-tick counter for the fire random-tick gate. +1 per
    /// net-tick frame. Q.4c uses `(pos, fire_clock)` as the seed
    /// for the deterministic 1/30 probability check that gates
    /// the spread + age step.
    pub(crate) fire_clock: u64,
    /// Per-furnace live runtime: remaining fuel ms + cook progress
    /// ms + the full fuel duration for the UI bar. Driven by
    /// `block::furnace::furnace_tick`; persisted alongside the
    /// slot vec.
    pub(crate) furnace_runtimes: HashMap<[i32; 3], block::furnace::FurnaceRuntime>,
    /// W172: wall-clock accumulator (ms) for the hopper transport
    /// cadence. Hoppers move one item per `HOPPER_INTERVAL_MS`.
    pub(crate) hopper_clock_ms: u32,
    /// Position of the furnace the player has open. Mirrors
    /// `chest_open`. Cleared by ESC / walking too far / opening
    /// another panel.
    pub(crate) furnace_open: Option<[i32; 3]>,
    /// Cursor-held stack while the furnace UI is open. Mirrors
    /// `chest_held`.
    pub(crate) furnace_held: Option<(u16, u8)>,
    /// W50.3: position of the crafting_table block the player has
    /// open. `None` means no 3x3 panel is visible. Client-side only -
    /// crafting tables aren't shared server state.
    pub(crate) crafting_table_open: Option<[i32; 3]>,
    /// W50.3: 3x3 crafting grid, row-major (idx = row*3 + col).
    /// Cleared when the player ESCs out / breaks the table - the
    /// table panel's "close" handler drops the contents back into
    /// inventory.
    pub(crate) craft_grid_3x3: [Option<(BlockId, u8)>; 9],
    /// W50.3: cursor-held stack while the crafting table UI is open.
    /// Mirrors `chest_held` / `furnace_held`.
    pub(crate) crafting_table_held: Option<(u16, u8)>,
    /// W171: open stonecutter block + its single input stack. Output
    /// is taken straight into the inventory (no floating cursor) so
    /// the interaction stays simple: click an inventory item to load
    /// the input, click a cut to craft.
    pub(crate) stonecutter_open: Option<[i32; 3]>,
    /// W319: villager the trade panel is open against (mob id).
    pub(crate) trade_open: Option<u32>,
    /// W341: session cache of claimed professions (mob id ->
    /// workstation-derived profession). Save persistence is a
    /// follow-up (MobSave field).
    pub(crate) mob_professions:
        std::collections::HashMap<u32, &'static str>,
    pub(crate) stonecutter_input: Option<(u16, u8)>,
    /// W410: open smithing table position. Session-only UI (the
    /// upgrade is an inventory transform; nothing persists in the
    /// world), exactly the stonecutter pattern.
    pub(crate) smithing_open: Option<[i32; 3]>,
    /// W410: smithing slots [template, base, addition].
    pub(crate) smithing_slots: [Option<(u16, u8)>; 3],
    /// W50.5: per-brewing-stand 5-slot container state, keyed by
    /// world voxel coords. Parallel to `furnaces`. Inserted on
    /// place, removed on break (drops fall as item entities).
    pub(crate) brewing_stands: HashMap<[i32; 3], container::Container>,
    /// W50.5: live runtime (progress_ms) per brewing stand.
    pub(crate) brewing_runtimes:
        HashMap<[i32; 3], block::brewing::BrewingRuntime>,
    /// W50.5: position of the brewing stand the player has open.
    pub(crate) brewing_open: Option<[i32; 3]>,
    /// W50.5: cursor-held stack while the brewing UI is open.
    pub(crate) brewing_held: Option<(u16, u8)>,
    /// W61.2: open-door positions. Stored as the LOWER-half voxel
    /// pos so two halves share one entry. RMB toggles via
    /// `block::door::toggle_door`; the render path checks via
    /// `block::door::is_open`.
    pub(crate) door_open: std::collections::HashSet<[i32; 3]>,
    /// W164: per-note-block pitch step (0..=24, F#3..F#5). RMB cycles
    /// it; absent = pitch 0. Cleared on world / dimension reset.
    pub(crate) note_pitches: std::collections::HashMap<[i32; 3], u8>,
    /// W125: per-door facing (0=+X,1=+Z,2=-X,3=-Z), keyed by the
    /// LOWER-half voxel. Set at place time from the placer's look
    /// direction; drives which cell edge the panel renders against
    /// (closed = opposite the facing, open = hinge-left swing).
    pub(crate) door_facing: std::collections::HashMap<[i32; 3], u8>,
    /// W143: stored rail shapes (MC `RailShape` block state), keyed
    /// by the rail voxel. Decided at PLACE time via the RailState
    /// port in `block::rail_state` - a junction with two established
    /// connections never re-bends when new rails appear nearby.
    /// Legacy rails from pre-W143 saves backfill on first remesh.
    pub(crate) rail_shapes: std::collections::HashMap<[i32; 3], u8>,
    /// W147: per-mob walk-animation state (MC WalkAnimationState),
    /// advanced by the render pass from observed positions. Purely
    /// visual - never saved or broadcast.
    pub(crate) mob_anim:
        std::collections::HashMap<u32, crate::entity_model::animation::MobAnim>,
    /// W151: live underwater bubbles (visual only; the textured
    /// particle pass is immediate-mode so risers keep state here).
    pub(crate) bubbles: Vec<crate::ambient::Bubble>,
    /// W153: textured fire + portal cell meshes (frame-0 crops of
    /// the MC animation strips; legacy tinted slabs when absent).
    pub(crate) effect_meshes: crate::render::effect_meshes::EffectMeshes,
    /// W134: live minecarts (offline/host authoritative; clients
    /// keep this empty and render `remote_carts`).
    pub(crate) carts: Vec<crate::minecart::Minecart>,
    /// W134: monotonic cart id source (host).
    pub(crate) next_cart_id: u32,
    /// W134: id of the cart the LOCAL player is riding.
    pub(crate) riding_cart: Option<u32>,
    /// W134: rider W/S thrust this frame (-1..1, captured by
    /// `update_play`, projected onto the rail by the cart tick).
    pub(crate) cart_input: f32,
    /// W134: Shift pressed while riding - dismount on the next tick.
    pub(crate) cart_dismount_pending: bool,
    /// W134: accumulator gating the host's ~10 Hz `vx.cart` stream.
    pub(crate) cart_net_accum: f32,
    /// W134: client-side view of the host's cart snapshots.
    pub(crate) remote_carts: std::collections::HashMap<u32, crate::net::CartMsg>,
    /// W134: baked minecart entity model.
    pub(crate) minecart_meshes: crate::render::minecarts::MinecartMeshes,
    /// W140: live arrows (host authoritative; clients render
    /// `remote_arrows`).
    pub(crate) arrows: Vec<crate::projectile::Arrow>,
    /// W140: monotonic arrow id source (host).
    pub(crate) next_arrow_id: u32,
    /// W343: per-creeper swell seconds (0..1.5); render reads the
    /// flash/inflate from here, the AI owns the state machine.
    pub(crate) mob_swell: std::collections::HashMap<u32, f32>,
    /// W348: mob id the local player is riding (horse/donkey;
    /// host/offline only - client mount is an MP follow-up).
    pub(crate) riding_mob: Option<u32>,
    /// W348: rider input in mount-local space [forward, strafe],
    /// already MC-scaled (back rows x0.25, sideways x0.5).
    pub(crate) riding_wish: [f32; 2],
    /// W348: space held - the AI consumes it on ground contact.
    pub(crate) riding_jump: bool,
    /// W338: live primed TNT (host authoritative).
    pub(crate) primed_tnt: Vec<crate::tnt::PrimedTnt>,
    pub(crate) next_tnt_id: u32,
    /// W338: client-side view of the host's TNT snapshots.
    pub(crate) remote_tnt:
        std::collections::HashMap<u32, crate::net::TntMsg>,
    /// W337: live ghast fireballs (host authoritative).
    pub(crate) fireballs: Vec<crate::projectile::Fireball>,
    pub(crate) next_fireball_id: u32,
    /// W337: client-side view of the host's fireball snapshots.
    pub(crate) remote_fireballs:
        std::collections::HashMap<u32, crate::net::FireballMsg>,
    /// W140: client-side view of the host's arrow snapshots.
    pub(crate) remote_arrows: std::collections::HashMap<u32, crate::net::ArrowMsg>,
    /// W140: baked arrow mesh.
    pub(crate) arrow_meshes: crate::render::projectiles::ArrowMeshes,
    /// MC-style per-cell tick schedule. `world_set_tracked` and
    /// the tick loop queue cells here with a `due_at_tick`; each
    /// frame pops due entries up to `MAX_LIQUID_PER_FRAME`. No
    /// hard cap on the schedule itself - cells that don't fit a
    /// frame keep their slot and tick on the next.
    ///
    /// Replaces the old `liquid_active: HashSet` sweep: the
    /// schedule's `pending` set serves the same "is this cell
    /// scheduled?" role.
    pub(crate) liquid_schedule: LiquidSchedule,
    /// Game-tick counter for the schedule. W104: advanced at a fixed
    /// 20 ticks/sec (MC game-tick rate) by `tick_liquids`, NOT once
    /// per render frame. Used both as `now` for `pop_due` and as the
    /// base for computing `due_at_tick = now + tick_delay`.
    pub(crate) liquid_clock: u64,
    /// W104: wall-clock accumulator (seconds) that gates `liquid_clock`
    /// advancement to a fixed 20 Hz so liquid flow speed is independent
    /// of frame rate. Transient - not persisted; reset on world load.
    pub(crate) liquid_tick_accum: f32,
    /// W198.7: same fixed-20 Hz accumulator for mob AI. Mob `step_one`
    /// (goals + walk + head-look) ran once per RENDER frame, so 25 mobs
    /// cost ~1ms each EVERY frame - framerate-dependent CPU that capped
    /// sim_ms (and was invisible in the F3 panel). MC ticks mobs at 20
    /// TPS; this gates `tick_mobs` to the same cadence. Transient.
    pub(crate) mob_tick_accum: f32,
    /// W207: ticks (in MC's 20 Hz scale) the eye has been submerged,
    /// clamped 0..=600 - drives MC's waterVision underwater-fog ramp
    /// (`render::sky::water_vision_mult`). +20/s underwater, -200/s
    /// surfaced, exactly LocalPlayer.waterVisionTime.
    pub(crate) water_vision_time: f32,
    /// W221: smoothed sky exposure at the camera (sky light / 15,
    /// eased over ~0.3s). Darkens the distance fog + clear colour
    /// underground - MC's caves fade to darkness, not to a glowing
    /// sky-coloured haze.
    pub(crate) cave_dark: f32,
    /// W204: every mob's position at the START of the last 20 Hz tick.
    /// The renderer lerps prev -> current by `mob_tick_accum /
    /// MOB_TICK_DT` so mobs glide between ticks instead of stepping
    /// 20x/s ("vibrating" at high FPS). Host-only (clients receive
    /// snapshots and keep their existing behaviour); transient.
    pub(crate) mob_prev_pos: HashMap<u32, [f32; 3]>,
    /// Every liquid voxel currently in the world. Maintained on
    /// every peer (host + clients) so the dedicated slope-mesh
    /// render path can iterate without scanning chunks. The
    /// block id at the world coord tells the mesher which
    /// liquid lives at that cell.
    pub(crate) liquid_positions: HashSet<[i32; 3]>,
    /// W198.10: the LAVA subset of `liquid_positions`. W267: keyed by
    /// CHUNK COLUMN `[cx, cz]` - the W260 deep lava sea put ~1M lava
    /// cells in the world, and the old flat set made every consumer
    /// (`tick_lava_fire_emit`, the ambient particle pass) an
    /// O(all-lava) per-tick scan again (5-13 ms each, the user's
    /// 10 FPS report). Consumers now look up only the columns within
    /// their reach. Rebuilt inside the liquid-mesh rescan (which
    /// already walks every liquid on change); entries re-validate
    /// against the world, so a momentarily stale cell is harmless.
    pub(crate) lava_columns: HashMap<[i32; 2], Vec<[i32; 3]>>,
    /// W64: cells queued for a falling-block check. A freshly placed
    /// falling block (sand / concrete_powder / gravel) and the cell
    /// directly above any removed block land here; the host-only
    /// `world::falling::tick` drops each unsupported one a voxel per
    /// tick. Host-only - clients never insert (their place / break
    /// hooks skip it) and the tick clears any stragglers.
    pub(crate) falling_pending: HashSet<[i32; 3]>,
    /// W65.3: player-placed leaf cells, exempt from random-tick decay
    /// (MC's `persistent` leaf flag). Tree-generated leaves (from
    /// worldgen and sapling growth) are absent and decay when no log
    /// remains in range. `world_set_tracked` drops an entry the moment
    /// its cell stops being a leaf.
    pub(crate) persistent_leaves: HashSet<[i32; 3]>,
    /// W67.1: levers currently in the "on" position. The redstone
    /// solver reads this as a constant power-15 source set; an RMB on
    /// a lever toggles membership. Server-side only (not persisted in
    /// v1 - levers reset off on reload).
    pub(crate) lever_on: HashSet<[i32; 3]>,
    /// MC-style level per liquid voxel: 0 = source, 1..=7 = flow
    /// (lower = closer to source). Missing entry defaults to 0
    /// (source) so loaders from old saves treat existing fluid
    /// as a source.
    pub(crate) liquid_level: HashMap<[i32; 3], u8>,
    /// Phase L (Wave 3): per-chunk nibble-packed light grids
    /// (block + sky) plus the dirty-voxel set drained by the
    /// lighting tick. Re-derived from the block grid on world
    /// load + dim switch - never persisted.
    pub(crate) lighting: lighting::LightingState,
    /// Seconds since the last lighting tick. The lighting tick
    /// runs at ~10 Hz (`LIGHTING_TICK_PERIOD_S`); intermediate
    /// frames just stamp the dirty set + the meshes keep
    /// rendering the previous tick's light values.
    pub(crate) last_lighting_tick_s: f32,
    /// Server-authoritative mob list (sheep + future kinds). Only
    /// populated in `NetRole::Server` / `NetRole::Offline`. Each
    /// tick the server updates pos / yaw + broadcasts `vx.mob`.
    pub(crate) mobs: Vec<MobState>,
    /// Client-side view of remote mobs: `id -> latest MobMsg`.
    /// In offline / server mode this stays in sync with `mobs` so
    /// the same render path works.
    pub(crate) remote_mobs: HashMap<u32, MobMsg>,
    /// Day-clock value of the last successful mob spawn. Used to
    /// throttle spawn attempts to one every `MOB_SPAWN_PERIOD`
    /// seconds.
    pub(crate) last_mob_spawn_time: f32,
    /// Day-clock value of the last despawn-pass check. H.6 runs
    /// the despawn scan every `DESPAWN_CHECK_PERIOD` seconds so
    /// the squared-distance loop doesn't drain per-tick CPU.
    pub(crate) last_despawn_check_time: f32,
    /// Counter for the next mob id. Reset to 1 because 0 is the
    /// host player's id in the pose table - no point in collisions.
    pub(crate) next_mob_id: u32,
    /// Per-mob step-sound cooldown (seconds). Drained while the
    /// mob is moving; sound fires + resets when it reaches zero.
    pub(crate) mob_step_timers: HashMap<u32, f32>,
    /// Per-mob baa/say cooldown (seconds). Decrements every tick;
    /// fire + roll a new random delay when it reaches zero.
    pub(crate) mob_say_timers: HashMap<u32, f32>,
    /// Last known (x, z) for each mob - used to detect movement
    /// for step-sound triggering on the client (the server doesn't
    /// broadcast velocity, so we infer from pos deltas).
    pub(crate) mob_last_xz: HashMap<u32, [f32; 2]>,
    /// Visual hurt-flash decay per mob (seconds remaining). Tinted
    /// red while > 0, fades each frame. Populated locally on the
    /// attacker + by `vx.mob_event` broadcasts on every other
    /// peer so everyone sees the flash.
    pub(crate) mob_hurt_flash: HashMap<u32, f32>,
    /// Seconds since each mob last took damage. Drives HP regen
    /// so mobs only heal when out of combat. Reset on every
    /// `damage_mob` hit, advanced each tick by `update_mobs`.
    pub(crate) mob_no_hit_time: HashMap<u32, f32>,
    /// Seconds remaining in the local death-fade animation per
    /// mob. Server populates from `MobState.dying` when a kill
    /// fires; client populates on `vx.mob_event { kind: 1 }`. The
    /// render path tilts + fades the cube while the timer counts
    /// down to zero.
    pub(crate) mob_dying: HashMap<u32, f32>,
    /// Per-mob lunge-animation timer. Set to `MOB_LUNGE_TIME` on
    /// every melee swing (currently only zombies). Renderer reads
    /// it to push the body briefly forward + return.
    pub(crate) mob_lunge: HashMap<u32, f32>,
    /// Cube mesh used to render every remote player as a coloured
    /// 1x1x1 marker. Built lazily once the renderer is up.
    pub(crate) player_cube: Option<MeshHandle>,
    /// Slope-aware mesh per liquid kind (one handle for water,
    /// one for lava, ...). The renderer draws each with its own
    /// PBR colour so different liquids can share the same
    /// geometry pipeline. Rebuilt lazily when
    /// `liquid_mesh_dirty` flips (place / fall / spread / decay
    /// / source merge).
    /// W111: PER-CHUNK liquid meshes (chunk coord -> one mesh per liquid
    /// kind in that chunk). Replaces the single whole-world mesh so only
    /// chunks whose liquid changed get rebuilt - no whole-world churn, no
    /// throttle, scales to oceans.
    /// W212: each entry carries its draw tint, computed at build time -
    /// water gets the chunk's MC biome water colour (warm ocean
    /// turquoise, swamp murk), lava stays white (its texture is fully
    /// coloured).
    pub(crate) liquid_mesh_handles: LiquidMeshes,
    /// W384: worker-meshed liquid chunks in flight - coord ->
    /// dispatch epoch. A landing result with a stale epoch is
    /// dropped (the chunk was re-dispatched after a change).
    pub(crate) liquid_mesh_inflight: HashMap<ChunkCoord, u64>,
    pub(crate) liquid_mesh_epoch: u64,
    pub(crate) liquid_mesh_dirty: bool,
    /// W111: last-meshed signature per chunk (commutative fold over the
    /// chunk's liquid cell positions + levels). The diff re-meshes only
    /// chunks whose signature changed. Transient, not persisted.
    pub(crate) liquid_chunk_sig: HashMap<ChunkCoord, u64>,
    /// W195: chunks queued for a liquid remesh, drained a few per frame.
    /// The expensive all-liquids signature scan runs ONCE when
    /// `liquid_mesh_dirty` is set and fills this; draining it on later
    /// frames needs no rescan, so a big settled ocean costs nothing per
    /// frame (the old code rescanned ~400k cells every frame while the
    /// mesh budget was backlogged - the "FPS zalostny u vody" bug).
    pub(crate) liquid_remesh_backlog: std::collections::HashSet<ChunkCoord>,
    /// W284: per-chunk-column MC biome water colour, so a busy liquid
    /// remesh backlog doesn't re-run the climate-noise biome sample
    /// for the same column every rebuild. Cleared on dim switch /
    /// world reset (biomes are per-dim + per-seed).
    pub(crate) water_tint_cache: HashMap<[i32; 2], [f32; 3]>,
    /// Worldgen variant the current chunks were generated from.
    /// Compared against `WorldGen::current()` on `Play offline`
    /// so the world rebuilds when the user picks a different
    /// variant in the main-menu combo. `None` until first
    /// `build_world` runs.
    pub(crate) last_built_worldgen: Option<WorldGen>,
    /// Floating drop entities the world (host-authoritative).
    /// Each tick the host steps physics, runs pickup, broadcasts
    /// spawn / pickup messages. Clients keep a mirrored Vec
    /// purely for rendering.
    pub(crate) dropped_items: Vec<DropItem>,
    /// Monotonic id source for new drops on the host. Clients
    /// keep at 0; ids come from the server.
    pub(crate) next_drop_id: u32,
    /// Cooldown timer for the running-water ambient SFX. Ticks
    /// down each frame the player stands in a flow voxel; the
    /// running-stream sting fires when it hits zero and resets.
    pub(crate) flow_sfx_accum: f32,
    /// Dedicated cube mesh for sheep bodies. Has its own
    /// `AlbedoHandle` binding so the texture (loaded from
    /// `assets/textures/entity/sheep/sheep.tga`) doesn't leak onto
    /// the player marker cubes.
    pub(crate) sheep_mesh: Option<MeshHandle>,
    /// Dedicated cube mesh for sheep heads, bound to a 16x16
    /// crop of the MC sheep entity texture (the face region).
    /// Falls back to flat-colour rendering on `player_cube` when
    /// the source `sheep.tga` is missing.
    pub(crate) sheep_head_mesh: Option<MeshHandle>,
    /// W19.2: per-body-part textured mesh handles for the 6 non-sheep mob
    /// kinds. Each `Option<MeshHandle>` is `None` until the entity texture
    /// for that kind is found on disk during `on_start`. The render loop
    /// falls back to the tinted `player_cube` when absent.
    pub(crate) mob_meshes: mob_meshes::MobMeshes,
    /// CPU particle system: short-lived debris on every block break.
    pub(crate) particles: infengine_particles::ParticleSystem,
    /// W18.3: textured billboard particles waiting to be flushed to
    /// the GPU particle pass at the start of the next render frame.
    /// Populated by `tick_ambient_particles` (flame/smoke/lava) and
    /// drained (cleared) at the end of each render call.
    pub(crate) textured_particles: Vec<infengine_render::TexturedParticleInstance>,
    /// Shared atlas bind group for the voxel chunk submeshes. Built
    /// once in `on_start`; every new chunk submesh clones the Arc
    /// instead of uploading a fresh texture.
    pub(crate) voxel_atlas: Option<infengine_render::AlbedoHandle>,
    /// Raw RGBA copy of the block atlas, retained so the egui UI
    /// can upload its own texture handle for slot icons (without
    /// re-reading the disk / re-running the procedural builder).
    pub(crate) atlas_rgba: Option<(u32, u32, Vec<u8>)>,
    /// egui texture handle for the block atlas. Loaded lazily on
    /// first inventory render so we don't pay the upload cost until
    /// the player actually opens the menu.
    pub(crate) egui_atlas: Option<egui::TextureHandle>,
    /// W157: per-item cached first-person held-item mesh. Built lazily
    /// the first time an item is held + reused thereafter.
    pub(crate) held_mesh_cache: crate::render::held_item::HeldMeshCache,
    /// W157.2: first-person swing + equip animation clocks.
    pub(crate) held_anim: crate::render::held_item::HeldAnim,
    /// W12.6: cached egui handle for the MC main-menu panorama
    /// (one of the 6 cube faces from
    /// `assets/minecraft/textures/gui/title/background/panorama_*.png`).
    /// Loaded lazily on the first menu draw + reused across frames;
    /// the underlying texture survives until the program exits.
    pub(crate) egui_panorama: Option<egui::TextureHandle>,
    /// W12.6: scroll offset accumulator for the panorama backdrop.
    /// Advanced each frame by `PANORAMA_ROT_RATE_RAD_PER_SEC * dt`
    /// and wrapped to `0..=1` so UV math stays numerically stable
    /// over long sessions.
    pub(crate) panorama_scroll: f32,
    /// Optional directory of PNG / TGA block textures. When set,
    /// `on_start` tries to load the atlas from there before
    /// falling back to the procedural builder.
    pub(crate) texture_dir: Option<std::path::PathBuf>,
    /// W9: MC resource pack loaded at startup. Carries the atlas
    /// uv lookup, item icons, sound event registry + the source
    /// path (shown on the settings page). Always present; uses
    /// the placeholder pack when no MC install was discovered.
    /// W24: wrapped in Arc so worker threads can clone a reference
    /// and read concurrently during parallel chunk meshing.
    pub(crate) resource_pack: Arc<resource_pack::ResourcePack>,
    /// Master-server endpoint the in-game browser queries. Settable
    /// via `--master <addr>` argv; defaults to the configured
    /// public master.
    pub(crate) master_addr: String,
    /// Buffers + state backing the three new menu screens. Lifted
    /// out of `AppState` so the enum stays Copy.
    ///
    /// Wave 4 retired the flat-picker `Host server...` flow, so the
    /// host buffers below are only reached from
    /// `start_hosting` / `announce_to_master` (both
    /// `#[allow(dead_code)]` for the same reason). The new MP screen
    /// has no Host UI yet; tracked as a deferred follow-up.
    #[allow(dead_code)]
    pub(crate) direct_addr_buf: String,
    pub(crate) host_name_buf: String,
    #[allow(dead_code)]
    pub(crate) host_port_buf: String,
    #[allow(dead_code)]
    pub(crate) host_announce: bool,
    pub(crate) browser_servers: Vec<infengine_net::master::ServerInfo>,
    pub(crate) browser_error: Option<String>,
    pub(crate) browser_last_refresh: Option<std::time::Instant>,
    /// In-flight master `ListServers` query. The refresh path now
    /// runs on a worker thread so the UI doesn't freeze for the
    /// 2-second master timeout. `Some` while a query is pending;
    /// cleared on result delivery.
    pub(crate) browser_query_rx:
        Option<std::sync::mpsc::Receiver<Result<Vec<infengine_net::master::ServerInfo>, String>>>,
    /// In-flight master `Register` query (host announce). Same
    /// async pattern as the browser refresh.
    pub(crate) announce_query_rx: Option<std::sync::mpsc::Receiver<Result<u64, String>>>,
    /// Returned by the master server on successful Register; the
    /// host echoes it back on Heartbeat + Unregister.
    pub(crate) master_token: Option<u64>,
    /// Last heartbeat we sent to the master (server mode). Drives
    /// the periodic re-heartbeat in `tick_net`.
    pub(crate) last_heartbeat: Option<std::time::Instant>,
    /// Buffer for the "New world" name input in the main-menu Worlds
    /// picker (Phase B.7a scaffolding). Wave 4 moved the input into
    /// `MenuScreen::Singleplayer::new_world_open` so this buffer is
    /// no longer read; kept as a default-empty field so a future
    /// chat-command (`/newworld <name>`) can route through the same
    /// `apply_menu_action(MenuAction::NewWorld { ... })` path.
    #[allow(dead_code)]
    pub(crate) new_world_name_buf: String,
    /// Wave 4 menu state machine + per-screen form buffers + cached
    /// `list_worlds()` snapshot. Drives every pre-Play UI screen.
    /// Initialised in `Voxel::new` so the very first frame has a
    /// valid screen to draw.
    pub(crate) menu: menu::Menu,
    /// Wave 7 plugin registry. `Some` while plugins are loaded;
    /// `None` between dispatcher calls (`take`-restore pattern).
    /// Stored as `Option` so the dispatcher can split-borrow it
    /// without aliasing `&mut Voxel` with `&mut PluginRegistry`.
    pub(crate) plugins: Option<plugin::PluginRegistry>,
    /// Wave 7: blocks plugins registered at runtime. Each entry
    /// has an id (1000..=1999, allocated by the engine), a
    /// display name + a tint colour. The `/plugins` chat
    /// command lists these.
    pub(crate) plugin_blocks: Vec<PluginBlock>,
    /// W28.9: Cached icon UV table - maps item id -> UV rect.
    /// Rebuilt only when the animation frame changes so we avoid
    /// ~200 HashMap inserts per frame on the UI hot path.
    pub(crate) icon_uv_table: HashMap<u16, [f32; 4]>,
    /// W28.9: The `app_time_ms / icon_anim_period_ms` value at which
    /// `icon_uv_table` was last built. When this changes we rebuild.
    pub(crate) icon_uv_table_frame: u32,
    /// W28.9: Preallocated Vec for the stream_chunks delivered buffer.
    /// Cleared each frame instead of allocating a new Vec.
    pub(crate) stream_delivered: Vec<WorkerResult>,
}

/// One block kind contributed by a plugin via
/// `Engine::block_register`. Wave 7 stores them on `Voxel`
/// alongside the engine's static `BLOCK_TYPES` table; future
/// work synthesises atlas tiles + extends the world meshing
/// path so these blocks actually render. `color` is parked for
/// the future renderer hookup; `/plugins` reads it next.
#[derive(Debug, Clone)]
pub(crate) struct PluginBlock {
    pub(crate) id: u16,
    pub(crate) name: String,
    #[allow(dead_code)]
    pub(crate) color: [f32; 3],
}

// W47.28 - impl Voxel block split into per-domain submodules above.
// W47.27 - Settings struct + Default impl (relocated from main.rs).
#[derive(Debug, Clone)]
pub(crate) struct Settings {
    pub(crate) mouse_sensitivity: f32,
    pub(crate) lock_cursor: bool,
    pub(crate) hide_cursor: bool,
    pub(crate) show_fps: bool,
    /// Render distance in chunks (Chebyshev). The unload radius is
    /// computed as `view_distance + 2` so the player can wobble on
    /// the boundary without thrashing chunk loads.
    pub(crate) view_distance: u8,
    /// Camera head bob while walking. Off-by-default-friendly:
    /// nauseating for some players, polish for others.
    pub(crate) head_bob: bool,
    /// Invert vertical mouse axis. Some players prefer down=look-up
    /// so leave it toggleable from the Settings panel.
    pub(crate) invert_mouse_y: bool,
    /// Reverse mouse-wheel hotbar scrolling direction.
    pub(crate) invert_wheel: bool,
    /// W9.7: optional override for the MC resource pack location.
    /// `None` = use the auto-discovered default (env var ->
    /// %APPDATA%/.minecraft -> placeholder). When set + the path
    /// resolves to a usable `assets/minecraft/` tree, the engine
    /// loads textures + sounds from there instead.
    pub(crate) mc_assets_path: Option<String>,
    /// W124: UI language code (MC lang file stem, e.g. "en_us",
    /// "cs_cz"). Drives item/block display names via `lang`;
    /// search always matches this language AND English.
    pub(crate) language: String,
    /// W292: GUI scale multiplier (MC "GUI Scale"). Applied as the
    /// egui zoom_factor, which COMPOSES with the OS DPI scale -
    /// 1.0 keeps the platform-native size, 2.0 doubles every
    /// panel/font (the 4K-monitor fix). Clamped to 0.5..=3.0.
    pub(crate) ui_scale: f32,
    /// W321: LOD ring radius in chunks (far surface shells beyond
    /// `view_distance`). 0 disables the ring; values at or below
    /// `view_distance` also effectively disable it.
    pub(crate) lod_distance: u8,
    /// W333: FAR ring radius in chunks (preliminary-surface tiles
    /// beyond `lod_distance`, up to 256). 0 disables it.
    pub(crate) lod2_distance: u8,
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
            language: "en_us".to_string(),
            ui_scale: 1.0,
            lod_distance: crate::lod::LOD_DISTANCE_DEFAULT,
            lod2_distance: crate::lod::LOD2_DISTANCE_DEFAULT,
        }
    }
}
