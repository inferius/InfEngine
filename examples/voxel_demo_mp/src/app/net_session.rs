//! W47.28 - net_session impl block (relocated from app/mod.rs).
#![allow(unused_imports)]
use crate::*;
use crate::lighting::HaloLighting;

impl Voxel {
    /// Spawn a NetClient at `addr` and transition into Playing.
    pub(crate) fn connect_as_client(&mut self, addr: &str) {
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
                self.mouse_dx = 0.0;
                self.mouse_dy = 0.0;
                // L.2: enter_world now sets AppState::Loading and
                // switches the menu to InGame itself.
                self.enter_world();
            }
            Err(err) => {
                warn!(error = %err, addr = %addr, "client connect failed");
                self.browser_error = Some(format!("connect failed: {err}"));
            }
        }
    }

    /// Bind a NetServer on the configured port, optionally announce
    /// to the master server, then drop into Playing.
    ///
    /// Wave 4 removed the "Host server..." UI button, so this is no
    /// longer reachable from clicks. Kept around as a runtime entry
    /// point a future chat-command (`/host`) or argv path can call;
    /// `#[allow(dead_code)]` silences the warning until then.
    #[allow(dead_code)]
    pub(crate) fn start_hosting(&mut self) {
        let port: u16 = self.host_port_buf.parse().unwrap_or(7777);
        let bind = format!("0.0.0.0:{port}");
        match NetServer::bind(&bind) {
            Ok(s) => {
                info!(addr = %bind, "hosting voxel server");
                // Pick a world seed for this session if one
                // wasn't restored from the on-disk state. The
                // nanosecond clock + a fixed mixing constant give
                // a different terrain layout per cold start.
                // W5.1: writes voxel.base_seed instead of the
                // (deleted) global atomic.
                if self.base_seed == 0 {
                    let seed = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.subsec_nanos())
                        .unwrap_or(0)
                        .wrapping_mul(0x9E37_79B1)
                        | 1;
                    self.base_seed = seed;
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
    ///
    /// Reachable from `start_hosting`; both are currently
    /// unreached-from-UI in Wave 4 (see the comment on
    /// `start_hosting` for the deferred follow-up).
    #[allow(dead_code)]
    pub(crate) fn announce_to_master(&mut self, port: u16) {
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
    pub(crate) fn refresh_browser(&mut self) {
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
    pub(crate) fn poll_master_queries(&mut self) {
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
    pub(crate) fn tick_net(&mut self) {
        crate::net::dispatch::tick(self);
    }
}
