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
use infengine_render::{MeshHandle, Renderer};
use infengine_voxel::{
    BlockDef, BlockId, BlockRegistry, CHUNK_SIZE, Chunk, ChunkCoord, ChunkSubmesh, RayHit, World,
    chunk_mesh, raycast,
};
use infengine_window::raw_winit::event::{ElementState, MouseButton, WindowEvent};
use infengine_window::{AppHandler, EngineWindow, FrameContext, Key, WindowConfig, run};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

/// One block-edit travelling over the wire. Sent as
/// `kind = "vx.edit"`. Server applies + broadcasts to every client
/// including the originator; clients apply on receive (don't predict
/// locally for v1).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct EditMsg {
    x: i32,
    y: i32,
    z: i32,
    block: u16,
}

/// Player pose snapshot. Sent every tick:
/// - client -> server as a single-player update (`id` ignored).
/// - server -> all clients as a per-player broadcast (with the
///   originator's `id`).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PlayerMsg {
    id: u32,
    pos: [f32; 3],
    yaw: f32,
    pitch: f32,
}

/// Active networking role of this binary. Owns the endpoint state +
/// per-tick broadcasting / receiving cadence.
enum NetRole {
    Offline,
    Server {
        server: NetServer,
        /// Latest pose every connected client has reported, plus the
        /// host's own under id 0. Broadcast every tick.
        poses: HashMap<u32, PlayerMsg>,
    },
    Client {
        client: NetClient,
        /// Latest snapshot of remote players (id != my own).
        remote: HashMap<u32, PlayerMsg>,
        /// Queue of block edits the client wants applied. Drained
        /// each tick and sent to the server as `vx.edit` payloads.
        pending_edits: Vec<EditMsg>,
    },
}

impl NetRole {
    fn label(&self) -> &'static str {
        match self {
            NetRole::Offline => "offline",
            NetRole::Server { .. } => "server",
            NetRole::Client { .. } => "client",
        }
    }
}

/// Initial chunk grid loaded eagerly before the player drops in. The
/// world streams more chunks in as the player walks beyond this
/// footprint.
const INITIAL_CHUNKS: i32 = 3;
/// Chunks within this Chebyshev distance of the player chunk are
/// kept loaded; new ones get queued for generation.
const VIEW_DISTANCE_CHUNKS: i32 = 4;
/// Chunks beyond this distance are unloaded (slightly larger than
/// `VIEW_DISTANCE_CHUNKS` so the player jiggling on the boundary
/// doesn't churn chunks every step).
const UNLOAD_DISTANCE_CHUNKS: i32 = 6;
/// Maximum number of worker results consumed per frame (each runs
/// `Renderer::create_mesh` for several submeshes - bounded so the
/// main thread doesn't stall on a sudden burst).
const RESULTS_PER_FRAME: usize = 4;
/// Number of background worker threads generating + meshing chunks.
const NUM_WORKERS: usize = 2;
const PLAYER_SPEED: f32 = 6.0;
/// Initial upward velocity on jump. Peak height = `v^2 / (2 * |g|)`,
/// so 8.0 ^ 2 / 36 ≈ 1.78 voxels - enough to clear a one-block step
/// with margin, similar to Minecraft's 1.25-block jump.
const JUMP_VELOCITY: f32 = 8.0;
const GRAVITY: f32 = -18.0;
const PLAYER_HEIGHT: f32 = 1.7;
const PLAYER_RADIUS: f32 = 0.3;
const COLLISION_EPSILON: f32 = 1e-4;
/// Hotbar slots: index 0 is the "break" slot (empty hand), the rest
/// hold the block ids in `Self::HOTBAR_BLOCKS`.
const HOTBAR_SLOTS: usize = 6;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum AppState {
    MainMenu,
    /// Direct-IP connect dialog: user types `host:port` and hits Join.
    DirectConnect,
    /// Browses servers fetched from the configured master server.
    ServerBrowser,
    /// Configures + spawns a host (game server). User can opt to
    /// announce on the master so others see it in the browser.
    HostSetup,
    Playing,
    Paused,
    Settings { back_to: BackTo },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum BackTo {
    MainMenu,
    Pause,
}

#[derive(Debug, Clone)]
struct Settings {
    mouse_sensitivity: f32,
    lock_cursor: bool,
    hide_cursor: bool,
    show_fps: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 0.6,
            lock_cursor: true,
            hide_cursor: true,
            show_fps: true,
        }
    }
}

struct Voxel {
    state: AppState,
    settings: Settings,
    requested_quit: bool,

    renderer: Option<Renderer>,
    ui: Option<EditorUi>,
    registry: BlockRegistry,
    world: World,
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
    /// crosshair / wireframe overlay and the click-to-edit step.
    target: Option<RayHit>,

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
    lmb_pending: bool,
    rmb_pending: bool,
    /// `None` = break slot (empty hand). `Some(block)` = place that
    /// block. Indexed by `selected_slot`.
    hotbar: [Option<BlockId>; HOTBAR_SLOTS],
    selected_slot: usize,
    /// Accumulated mouse-wheel ticks since the last tick consume; the
    /// sign drives slot selection.
    wheel_accum: f32,
    /// `true` when the player's feet sit on a solid voxel; resets
    /// every frame after collision resolution.
    grounded: bool,

    viewport: (u32, u32),
    fps_ema: f32,

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
    /// Cube mesh used to render every remote player as a coloured
    /// 1x1x1 marker. Built lazily once the renderer is up.
    player_cube: Option<MeshHandle>,
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
}

impl Voxel {
    fn new_with_net(net: NetRole) -> Self {
        let mut v = Self::new();
        v.net = net;
        v
    }

    /// Spawn a NetClient at `addr` and transition into Playing.
    fn connect_as_client(&mut self, addr: &str) {
        match NetClient::connect(addr, "voxel-mp-client") {
            Ok(c) => {
                info!(addr = %addr, "joining server");
                self.net = NetRole::Client {
                    client: c,
                    remote: HashMap::new(),
                    pending_edits: Vec::new(),
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
                self.net = NetRole::Server {
                    server: s,
                    poses: HashMap::new(),
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
        let player_pos = self.player_pos;
        let player_yaw = self.player_yaw;
        let player_pitch = self.player_pitch;
        // Server-side heartbeat to the master server. Cheap so we
        // can do it inline.
        if let NetRole::Server { server, poses } = &self.net
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
        let cfg = bincode::config::standard();
        match &mut self.net {
            NetRole::Offline => return,
            NetRole::Server { server, poses } => {
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
                for (client_id, kind, data) in server.drain_bytes() {
                    match kind.as_str() {
                        "vx.edit" => {
                            if let Ok((e, _)) =
                                bincode::serde::decode_from_slice::<EditMsg, _>(&data, cfg)
                            {
                                to_apply.push(e);
                            }
                        }
                        "vx.player" => {
                            if let Ok((mut p, _)) =
                                bincode::serde::decode_from_slice::<PlayerMsg, _>(&data, cfg)
                            {
                                p.id = client_id;
                                poses.insert(client_id, p);
                            }
                        }
                        _ => {}
                    }
                }
                // Catch-up: any client id that wasn't in last tick's
                // roster gets the entire edit history rebroadcast so
                // its locally-generated world matches the server's.
                // Existing clients receive duplicate edits but each
                // edit is an idempotent voxel set, so the redundant
                // traffic is correctness-safe.
                let current_ids: HashSet<u32> = poses
                    .keys()
                    .copied()
                    .filter(|id| *id != 0)
                    .collect();
                let new_ids: Vec<u32> = current_ids
                    .iter()
                    .filter(|id| !self.known_client_ids.contains(*id))
                    .copied()
                    .collect();
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
                self.known_client_ids = current_ids;
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
            }
            NetRole::Client {
                client,
                remote,
                pending_edits,
            } => {
                client.poll();
                let my_id = client.client_id;
                // Sweep own id out of the remote table - if a
                // pre-handshake broadcast snuck our pose in before
                // we knew our own id, drop it now.
                if let Some(mine) = my_id {
                    remote.remove(&mine);
                }
                for (kind, data) in client.drain_bytes() {
                    match kind.as_str() {
                        "vx.edit" => {
                            if let Ok((e, _)) =
                                bincode::serde::decode_from_slice::<EditMsg, _>(&data, cfg)
                            {
                                // Server-echoed edit: retire any
                                // pending request that matches it
                                // by coordinate + block. Idempotent
                                // dups are cheap; this just stops
                                // the retry-resend loop.
                                if let Some(idx) = pending_edits.iter().position(|p| {
                                    p.x == e.x
                                        && p.y == e.y
                                        && p.z == e.z
                                        && p.block == e.block
                                }) {
                                    pending_edits.remove(idx);
                                }
                                to_apply.push(e);
                            }
                        }
                        "vx.player" => {
                            if let Ok((p, _)) =
                                bincode::serde::decode_from_slice::<PlayerMsg, _>(&data, cfg)
                                && my_id != Some(p.id)
                            {
                                remote.insert(p.id, p);
                            }
                        }
                        _ => {}
                    }
                }
                // Re-send every pending edit each tick - UDP loss
                // on the upstream path eventually catches a retry.
                // Server applies idempotently so dups are free.
                for e in pending_edits.iter() {
                    if let Ok(buf) = bincode::serde::encode_to_vec(e, cfg) {
                        let _ = client.send_bytes("vx.edit", buf);
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
            self.world.set(voxel, BlockId(e.block));
            let coords = Self::affected_chunks(voxel);
            self.rebuild_chunks(&coords);
        }
    }

    fn new() -> Self {
        let mut registry = BlockRegistry::new();
        registry.insert(BlockId(1), BlockDef::solid("stone", [0.50, 0.50, 0.55]));
        registry.insert(BlockId(2), BlockDef::solid("dirt", [0.45, 0.32, 0.20]));
        registry.insert(BlockId(3), BlockDef::solid("grass", [0.35, 0.62, 0.28]));
        registry.insert(BlockId(4), BlockDef::solid("sand", [0.92, 0.86, 0.62]));
        registry.insert(BlockId(5), BlockDef::solid("wood", [0.55, 0.38, 0.22]));
        Self {
            state: AppState::MainMenu,
            settings: Settings::default(),
            requested_quit: false,
            renderer: None,
            ui: None,
            registry,
            world: World::new(),
            chunk_meshes: Vec::new(),
            pending_chunks: VecDeque::new(),
            pending_set: HashSet::new(),
            workers: None,
            target: None,
            player_pos: Vec3::new(0.0, 32.0, 0.0),
            player_vel: Vec3::ZERO,
            player_yaw: 0.0,
            player_pitch: -0.4,
            has_focus: true,
            last_mouse: None,
            mouse_dx: 0.0,
            mouse_dy: 0.0,
            esc_prev: false,
            lmb_pending: false,
            rmb_pending: false,
            hotbar: [
                None,
                Some(BlockId(1)),
                Some(BlockId(2)),
                Some(BlockId(3)),
                Some(BlockId(4)),
                Some(BlockId(5)),
            ],
            selected_slot: 0,
            wheel_accum: 0.0,
            grounded: false,
            viewport: (1, 1),
            fps_ema: 60.0,
            net: NetRole::Offline,
            player_cube: None,
            edit_history: Vec::new(),
            known_client_ids: HashSet::new(),
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
        }
    }

    /// Synchronously generate the initial chunk grid so the player
    /// drops into a non-empty world. Streaming workers take over once
    /// the player starts moving.
    fn build_world(&mut self) {
        for cz in -INITIAL_CHUNKS..=INITIAL_CHUNKS {
            for cx in -INITIAL_CHUNKS..=INITIAL_CHUNKS {
                let cc: ChunkCoord = [cx, 0, cz];
                self.world.insert_chunk(cc, generate_chunk_at(cc));
            }
        }
        self.rebuild_all_meshes();
        info!(
            chunks = self.world.iter_chunks().count(),
            submeshes = self.chunk_meshes.len(),
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

        // Queue missing chunks inside the view radius.
        for dz in -VIEW_DISTANCE_CHUNKS..=VIEW_DISTANCE_CHUNKS {
            for dx in -VIEW_DISTANCE_CHUNKS..=VIEW_DISTANCE_CHUNKS {
                let cc = [player_chunk[0] + dx, player_chunk[1], player_chunk[2] + dz];
                if self.world.chunk(cc).is_none() && self.pending_set.insert(cc) {
                    self.pending_chunks.push_back(cc);
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
                dx > UNLOAD_DISTANCE_CHUNKS || dz > UNLOAD_DISTANCE_CHUNKS
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
        if let Some(pool) = self.workers.as_ref() {
            while let Some(cc) = self.pending_chunks.pop_front() {
                if pool.req_tx.send(cc).is_err() {
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
            if dx > UNLOAD_DISTANCE_CHUNKS || dz > UNLOAD_DISTANCE_CHUNKS {
                continue;
            }
            if self.world.chunk(result.coord).is_some() {
                continue;
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
                for sub in result.submeshes {
                    let handle = renderer.create_mesh(
                        &format!("voxel_chunk_{}_{}_{}_b{}", cc[0], cc[1], cc[2], sub.block.0),
                        &sub.vertices,
                        &sub.indices,
                    );
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
        for coord in coords {
            let Some(chunk) = self.world.chunk(*coord) else {
                continue;
            };
            let origin = World::chunk_world_origin(*coord);
            for sub in chunk_mesh(chunk, origin) {
                let handle = renderer.create_mesh(
                    &format!(
                        "voxel_chunk_{}_{}_{}_b{}",
                        coord[0], coord[1], coord[2], sub.block.0
                    ),
                    &sub.vertices,
                    &sub.indices,
                );
                self.chunk_meshes.push((*coord, sub.block, handle));
            }
        }
    }

    /// Walk every chunk in the world and register its mesh handles.
    /// Used at startup. Edits use `rebuild_chunks` for the affected
    /// chunk + its solid neighbours so the cost stays local.
    fn rebuild_all_meshes(&mut self) {
        self.chunk_meshes.clear();
        let renderer = self.renderer.as_mut().expect("renderer ready");
        let coords: Vec<ChunkCoord> = self.world.iter_chunks().map(|(c, _)| *c).collect();
        for coord in coords {
            let chunk = self.world.chunk(coord).expect("chunk present");
            let origin = World::chunk_world_origin(coord);
            let subs = chunk_mesh(chunk, origin);
            for sub in subs {
                let handle = renderer.create_mesh(
                    &format!(
                        "voxel_chunk_{}_{}_{}_b{}",
                        coord[0], coord[1], coord[2], sub.block.0
                    ),
                    &sub.vertices,
                    &sub.indices,
                );
                self.chunk_meshes.push((coord, sub.block, handle));
            }
        }
    }

    fn apply_cursor_settings(&self, window: &EngineWindow) {
        use infengine_window::raw_winit::window::CursorGrabMode;
        let playing = matches!(self.state, AppState::Playing);
        let raw = window.raw();
        // Only grab when we have OS focus. Trying to grab an
        // unfocused window on Windows silently drops the lock and
        // can teleport the cursor onto a sibling window mid-click.
        let active = playing && self.has_focus;
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
        let dx = std::mem::take(&mut self.mouse_dx);
        let dy = std::mem::take(&mut self.mouse_dy);
        let sens = self.settings.mouse_sensitivity * 0.0035;
        self.player_yaw -= dx * sens;
        self.player_pitch -= dy * sens;
        let limit = std::f32::consts::FRAC_PI_2 - 0.05;
        self.player_pitch = self.player_pitch.clamp(-limit, limit);

        // Consume mouse wheel ticks - cycle hotbar slots (wheel up =
        // previous slot, matches Minecraft convention).
        let wheel = std::mem::take(&mut self.wheel_accum);
        let ticks = wheel.round() as i32;
        if ticks != 0 {
            let mut slot = self.selected_slot as i32 - ticks;
            slot = slot.rem_euclid(HOTBAR_SLOTS as i32);
            self.selected_slot = slot as usize;
        }

        let forward_horizontal =
            Vec3::new(self.player_yaw.cos(), 0.0, -self.player_yaw.sin()).normalize();
        // Right vector via the standard 90-deg horizontal rotation of
        // forward. For yaw=0 (forward = +X), right = +Z which matches
        // a player facing east strafing south.
        let right = Vec3::new(-forward_horizontal.z, 0.0, forward_horizontal.x);

        let mut move_axis = 0.0;
        if ctx.input.is_pressed(Key::W) {
            move_axis += 1.0;
        }
        if ctx.input.is_pressed(Key::S) {
            move_axis -= 1.0;
        }
        let mut strafe = 0.0;
        if ctx.input.is_pressed(Key::A) {
            strafe -= 1.0;
        }
        if ctx.input.is_pressed(Key::D) {
            strafe += 1.0;
        }
        let horizontal =
            (forward_horizontal * move_axis + right * strafe).normalize_or_zero() * PLAYER_SPEED;
        self.player_vel.x = horizontal.x;
        self.player_vel.z = horizontal.z;

        if ctx.input.is_pressed(Key::Space) && self.grounded {
            self.player_vel.y = JUMP_VELOCITY;
            self.grounded = false;
        }
        self.player_vel.y += GRAVITY * dt;

        // Axis-resolved voxel AABB collision.
        let delta = self.player_vel * dt;
        let (new_pos, hit_floor, hit_ceiling) = sweep_aabb(&self.world, self.player_pos, delta);
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

        if self.player_pos.y < -20.0 {
            // Fell through the world - respawn above origin.
            self.player_pos = Vec3::new(0.0, 32.0, 0.0);
            self.player_vel = Vec3::ZERO;
        }
    }

    fn render_ui(&mut self, window: &EngineWindow) {
        let state = self.state;
        let fps = self.fps_ema;
        let show_fps = self.settings.show_fps;
        let settings_edit = &mut self.settings;
        let hotbar_snapshot: Vec<Option<[f32; 3]>> = self
            .hotbar
            .iter()
            .map(|slot| slot.and_then(|b| self.registry.get(b).map(|d| d.color)))
            .collect();
        let selected_slot = self.selected_slot;
        let mut action_start = false;
        let mut action_resume = false;
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
        let direct_buf = &mut self.direct_addr_buf;
        let host_name = &mut self.host_name_buf;
        let host_port = &mut self.host_port_buf;
        let host_announce = &mut self.host_announce;
        let master_addr = &mut self.master_addr;
        let browser_servers = &self.browser_servers;
        let browser_error = &self.browser_error;
        let browser_last_refresh = self.browser_last_refresh;
        let browser_querying = self.browser_query_rx.is_some();

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
                    });
                });
            }
            AppState::Settings { .. } => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.heading("Settings");
                    ui.add(
                        egui::Slider::new(&mut settings_edit.mouse_sensitivity, 0.1..=2.0)
                            .text("Mouse sensitivity"),
                    );
                    ui.checkbox(&mut settings_edit.show_fps, "Show FPS");
                    ui.checkbox(
                        &mut settings_edit.lock_cursor,
                        "Lock cursor in window (Play)",
                    );
                    ui.checkbox(&mut settings_edit.hide_cursor, "Hide cursor (Play)");
                    ui.separator();
                    if ui.button("Back").clicked() {
                        action_back_settings = true;
                    }
                });
            }
            AppState::Playing => {
                egui::Area::new(egui::Id::new("voxel_hud"))
                    .anchor(egui::Align2::LEFT_TOP, [12.0, 12.0])
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.label("Mouse aim, WASD, Space jump, Esc menu");
                            ui.label("LMB break, RMB place");
                            if show_fps {
                                ui.separator();
                                ui.label(format!("FPS: {fps:.0}"));
                            }
                        });
                    });
                // Bottom hotbar - empty slot (= break) + one slot per
                // registered block. Highlighted slot is the active
                // selection (driven by mouse wheel).
                egui::Area::new(egui::Id::new("voxel_hotbar"))
                    .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -16.0])
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for (i, slot) in hotbar_snapshot.iter().enumerate() {
                                    let size = egui::vec2(40.0, 40.0);
                                    let (rect, _resp) =
                                        ui.allocate_exact_size(size, egui::Sense::hover());
                                    let painter = ui.painter_at(rect);
                                    let fill = match slot {
                                        None => egui::Color32::from_gray(24),
                                        Some(c) => egui::Color32::from_rgb(
                                            (c[0] * 255.0) as u8,
                                            (c[1] * 255.0) as u8,
                                            (c[2] * 255.0) as u8,
                                        ),
                                    };
                                    painter.rect_filled(rect, 4.0, fill);
                                    let outline = if i == selected_slot {
                                        egui::Stroke::new(
                                            3.0,
                                            egui::Color32::from_rgb(255, 220, 80),
                                        )
                                    } else {
                                        egui::Stroke::new(1.0, egui::Color32::from_gray(120))
                                    };
                                    painter.rect_stroke(
                                        rect,
                                        4.0,
                                        outline,
                                        egui::StrokeKind::Inside,
                                    );
                                    if slot.is_none() {
                                        // Draw an "X" to mark the
                                        // break (empty hand) slot.
                                        let pad = 10.0;
                                        let stroke =
                                            egui::Stroke::new(2.0, egui::Color32::from_gray(190));
                                        painter.line_segment(
                                            [
                                                egui::pos2(rect.min.x + pad, rect.min.y + pad),
                                                egui::pos2(rect.max.x - pad, rect.max.y - pad),
                                            ],
                                            stroke,
                                        );
                                        painter.line_segment(
                                            [
                                                egui::pos2(rect.max.x - pad, rect.min.y + pad),
                                                egui::pos2(rect.min.x + pad, rect.max.y - pad),
                                            ],
                                            stroke,
                                        );
                                    }
                                }
                            });
                        });
                    });

                // Crosshair drawn directly on the egui screen layer.
                // Two thin segments through the screen centre.
                let screen = ctx.content_rect();
                let center = screen.center();
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("voxel_crosshair"),
                ));
                let stroke = egui::Stroke::new(2.0, egui::Color32::from_rgb(240, 240, 240));
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
        });

        if action_start {
            self.state = AppState::Playing;
            self.mouse_dx = 0.0;
            self.mouse_dy = 0.0;
        }
        if action_resume {
            self.state = AppState::Playing;
            self.mouse_dx = 0.0;
            self.mouse_dy = 0.0;
        }
        if action_quit {
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
    }
}

impl AppHandler for Voxel {
    fn on_start(&mut self, window: &EngineWindow) {
        let (w, h) = window.size();
        self.viewport = (w, h);
        info!(width = w, height = h, "voxel_demo started");
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
        self.workers = Some(WorkerPool::spawn(NUM_WORKERS));
        self.build_world();
    }

    fn on_window_event(&mut self, window: &EngineWindow, event: &WindowEvent) {
        if let Some(ui) = self.ui.as_mut() {
            ui.handle_engine_event(window, event);
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
            && matches!(state, ElementState::Pressed)
        {
            match button {
                MouseButton::Left => self.lmb_pending = true,
                MouseButton::Right => self.rmb_pending = true,
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
        self.fps_ema = self.fps_ema * 0.9 + (1.0 / dt) * 0.1;

        // Pull any pending master-query results back onto the main
        // thread (refresh + announce both run on background workers).
        self.poll_master_queries();

        // Esc cycles Play <-> Paused and Settings -> back. Never quits.
        let esc_now = ctx.input.is_pressed(Key::Escape);
        let esc_just = esc_now && !self.esc_prev;
        self.esc_prev = esc_now;
        if esc_just {
            match self.state {
                AppState::Playing => self.state = AppState::Paused,
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

        if matches!(self.state, AppState::Playing) {
            self.update_play(ctx, dt);
            self.stream_chunks();
        } else {
            self.mouse_dx = 0.0;
            self.mouse_dy = 0.0;
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
        // player is currently aiming at.
        self.target = if matches!(self.state, AppState::Playing) {
            raycast(&self.world, self.player_pos, forward, 6.0)
        } else {
            None
        };

        // Apply pending mouse-button edits before rendering. LMB
        // always breaks the targeted voxel. RMB places the selected
        // hotbar block at the adjacent empty cell, but is ignored
        // when the empty (break) slot is selected.
        let lmb = self.lmb_pending;
        let rmb = self.rmb_pending;
        self.lmb_pending = false;
        self.rmb_pending = false;
        let selected_block = self.hotbar[self.selected_slot];
        // Net mode flag: clients defer block edits to the server +
        // wait for the server echo before applying. Offline + server
        // apply locally as before.
        let is_client = matches!(self.net, NetRole::Client { .. });
        if let Some(hit) = self.target {
            if lmb {
                let edit = EditMsg {
                    x: hit.voxel[0],
                    y: hit.voxel[1],
                    z: hit.voxel[2],
                    block: 0,
                };
                if is_client {
                    // Client-side prediction: apply immediately for
                    // snappy feedback, queue for the server. The
                    // server's echo will re-apply (idempotent set);
                    // a future rollback layer would reconcile on a
                    // mismatch.
                    self.world.set(hit.voxel, BlockId::AIR);
                    let coords = Self::affected_chunks(hit.voxel);
                    self.rebuild_chunks(&coords);
                    if let NetRole::Client { pending_edits, .. } = &mut self.net {
                        pending_edits.push(edit);
                    }
                } else {
                    self.world.set(hit.voxel, BlockId::AIR);
                    let coords = Self::affected_chunks(hit.voxel);
                    self.rebuild_chunks(&coords);
                    if let NetRole::Server { server, .. } = &self.net {
                        let cfg = bincode::config::standard();
                        if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                            server.broadcast_bytes("vx.edit", buf);
                        }
                    }
                    self.edit_history.push(edit);
                }
            } else if rmb && let Some(block) = selected_block {
                // Reject placement when it would intersect the player
                // capsule - otherwise the player gets stuck inside
                // the freshly placed block.
                let candidate = self.player_pos; // current eye pos
                let test_pos = candidate;
                let (min, max) = body_aabb(test_pos);
                let p = hit.place_at;
                let voxel_min = [p[0] as f32, p[1] as f32, p[2] as f32];
                let voxel_max = [p[0] as f32 + 1.0, p[1] as f32 + 1.0, p[2] as f32 + 1.0];
                let overlaps = max.x > voxel_min[0]
                    && min.x < voxel_max[0]
                    && max.y > voxel_min[1]
                    && min.y < voxel_max[1]
                    && max.z > voxel_min[2]
                    && min.z < voxel_max[2];
                if !overlaps {
                    let edit = EditMsg {
                        x: hit.place_at[0],
                        y: hit.place_at[1],
                        z: hit.place_at[2],
                        block: block.0,
                    };
                    if is_client {
                        self.world.set(hit.place_at, block);
                        let coords = Self::affected_chunks(hit.place_at);
                        self.rebuild_chunks(&coords);
                        if let NetRole::Client { pending_edits, .. } = &mut self.net {
                            pending_edits.push(edit);
                        }
                    } else {
                        self.world.set(hit.place_at, block);
                        let coords = Self::affected_chunks(hit.place_at);
                        self.rebuild_chunks(&coords);
                        if let NetRole::Server { server, .. } = &self.net {
                            let cfg = bincode::config::standard();
                            if let Ok(buf) = bincode::serde::encode_to_vec(edit, cfg) {
                                server.broadcast_bytes("vx.edit", buf);
                            }
                        }
                        self.edit_history.push(edit);
                    }
                }
            }
        }
        // Net tick: poll + apply + broadcast / send.
        self.tick_net();

        let player_pos = self.player_pos;
        let chunk_meshes = self.chunk_meshes.clone();
        let registry = &self.registry;
        let target = self.target;
        let ui = self.ui.as_mut();
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        {
            let cam = renderer.camera_3d_mut();
            cam.position = player_pos;
            cam.target = player_pos + forward;
        }
        renderer.set_clear_color(0.50, 0.70, 0.92, 1.0);
        // Render remote players as coloured cubes so each peer is
        // visible. Server uses its own `poses` map (skipping id 0 -
        // that's the host's own pose); client uses the `remote`
        // table (already strips its own id).
        let remote_iter: Vec<PlayerMsg> = match &self.net {
            NetRole::Offline => Vec::new(),
            NetRole::Server { poses, .. } => {
                poses.values().filter(|p| p.id != 0).copied().collect()
            }
            NetRole::Client { remote, .. } => remote.values().copied().collect(),
        };
        if !remote_iter.is_empty() {
            let cube = self.player_cube.get_or_insert_with(|| renderer.cube_handle());
            for p in remote_iter {
                let m = Mat4::from_scale_rotation_translation(
                    Vec3::new(0.6, 1.7, 0.6),
                    glam::Quat::from_rotation_y(p.yaw),
                    Vec3::new(p.pos[0], p.pos[1] - 0.85, p.pos[2]),
                );
                // Hash id to a stable colour.
                let h = (p.id.wrapping_mul(2654435761)) as f32 / u32::MAX as f32;
                let tau = std::f32::consts::TAU;
                let color = [
                    (0.4 + 0.5 * (h * tau).sin().abs()).min(1.0),
                    (0.4 + 0.5 * ((h + 0.33) * tau).sin().abs()).min(1.0),
                    (0.4 + 0.5 * ((h + 0.66) * tau).sin().abs()).min(1.0),
                    1.0,
                ];
                renderer.draw_mesh_pbr(*cube, m.to_cols_array(), color, 0.0, 0.7);
            }
        }
        let model = Mat4::IDENTITY.to_cols_array();
        for (_coord, block, handle) in &chunk_meshes {
            if let Some(def) = registry.get(*block) {
                renderer.draw_mesh_pbr(
                    *handle,
                    model,
                    [def.color[0], def.color[1], def.color[2], 1.0],
                    def.metallic,
                    def.roughness,
                );
            }
        }
        // Wire outline previewing the next action.
        //   Empty slot     -> wire around the voxel that will be
        //                     broken (target.voxel, black stroke).
        //   Block slot     -> wire around the cell where the block
        //                     will be placed (target.place_at,
        //                     tinted with the block's albedo).
        if let Some(hit) = target {
            let (cell, color) = match selected_block {
                None => (hit.voxel, [0.0, 0.0, 0.0, 0.95]),
                Some(block) => {
                    let c = registry
                        .get(block)
                        .map(|d| {
                            [
                                (d.color[0] * 1.4).min(1.0),
                                (d.color[1] * 1.4).min(1.0),
                                (d.color[2] * 1.4).min(1.0),
                                0.95,
                            ]
                        })
                        .unwrap_or([1.0, 1.0, 1.0, 0.95]);
                    (hit.place_at, c)
                }
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

/// Thread pool that runs `generate_chunk` + `chunk_mesh` off the main
/// thread. The main thread enqueues `ChunkCoord` requests on
/// `req_tx`; workers produce `WorkerResult` on `res_rx`. Dropping the
/// pool closes `req_tx`; the workers see `recv` return Err and exit.
struct WorkerPool {
    req_tx: Sender<ChunkCoord>,
    res_rx: Receiver<WorkerResult>,
    threads: Vec<JoinHandle<()>>,
}

impl WorkerPool {
    fn spawn(num_workers: usize) -> Self {
        let (req_tx, req_rx) = mpsc::channel::<ChunkCoord>();
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
                            // next coord. The lock is held only across one
                            // recv() call; generation runs lock-free.
                            let coord = {
                                let rx = req_rx.lock().expect("worker mutex");
                                match rx.recv() {
                                    Ok(c) => c,
                                    Err(_) => return,
                                }
                            };
                            let chunk = generate_chunk_at(coord);
                            let origin = World::chunk_world_origin(coord);
                            let submeshes = chunk_mesh(&chunk, origin);
                            if res_tx
                                .send(WorkerResult {
                                    coord,
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
        let (dummy_tx, _) = mpsc::channel::<ChunkCoord>();
        let _ = std::mem::replace(&mut self.req_tx, dummy_tx);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

/// Free-standing generator the workers call. Independent of any `Self`
/// so it ports cleanly into the worker closure.
fn generate_chunk_at(coord: ChunkCoord) -> Chunk {
    let origin = World::chunk_world_origin(coord);
    let height_at = |wx: f32, wz: f32| -> i32 {
        let h = 8.0
            + (wx * 0.13).sin() * 2.5
            + (wz * 0.17).cos() * 2.0
            + ((wx + wz) * 0.07).sin() * 1.5;
        h.round() as i32
    };
    let mut chunk = Chunk::empty();
    chunk.fill_with(|x, y, z| {
        let wx = origin.x + x as f32;
        let wz = origin.z + z as f32;
        let h = height_at(wx, wz);
        let wy = origin.y as i32 + y as i32;
        if wy > h {
            return BlockId::AIR;
        }
        if wy == h {
            BlockId(3)
        } else if wy >= h - 3 {
            BlockId(2)
        } else {
            BlockId(1)
        }
    });
    chunk
}

/// Player AABB at `pos`, where `pos.y` is the eye (head) height.
fn body_aabb(pos: Vec3) -> (Vec3, Vec3) {
    (
        Vec3::new(
            pos.x - PLAYER_RADIUS,
            pos.y - PLAYER_HEIGHT,
            pos.z - PLAYER_RADIUS,
        ),
        Vec3::new(pos.x + PLAYER_RADIUS, pos.y, pos.z + PLAYER_RADIUS),
    )
}

/// Returns true if `pos`'s AABB overlaps any solid voxel.
fn aabb_intersects_solid(world: &World, pos: Vec3) -> bool {
    let (min, max) = body_aabb(pos);
    let x0 = min.x.floor() as i32;
    let x1 = (max.x - COLLISION_EPSILON).floor() as i32;
    let y0 = min.y.floor() as i32;
    let y1 = (max.y - COLLISION_EPSILON).floor() as i32;
    let z0 = min.z.floor() as i32;
    let z1 = (max.z - COLLISION_EPSILON).floor() as i32;
    for y in y0..=y1 {
        for z in z0..=z1 {
            for x in x0..=x1 {
                if !world.get([x, y, z]).is_air() {
                    return true;
                }
            }
        }
    }
    false
}

/// Move the player AABB by `delta`, resolving collision with solid
/// voxels along each axis separately. Returns the new position, plus
/// flags indicating whether the player landed on a floor (negative
/// Y resolve) or bonked their head (positive Y resolve).
fn sweep_aabb(world: &World, pos: Vec3, delta: Vec3) -> (Vec3, bool, bool) {
    let mut new_pos = pos;
    let mut hit_floor = false;
    let mut hit_ceiling = false;

    // X axis.
    let candidate = Vec3::new(new_pos.x + delta.x, new_pos.y, new_pos.z);
    if aabb_intersects_solid(world, candidate) {
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
    if aabb_intersects_solid(world, candidate) {
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
    if aabb_intersects_solid(world, candidate) {
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
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--server" => {
                let addr = args.next().unwrap_or_else(|| "0.0.0.0:7777".to_string());
                match NetServer::bind(&addr) {
                    Ok(s) => {
                        info!(addr = %addr, "voxel server bound");
                        title_suffix = format!(" [SERVER {addr}]");
                        net = NetRole::Server {
                            server: s,
                            poses: HashMap::new(),
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
    if let Err(err) = run(config, Voxel::new_with_net(net)) {
        error!(error = %err, "event loop failed");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
