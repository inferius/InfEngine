//! UDP-based networking for InfEngine projects.
//!
//! Phase 1 ships a deliberately minimal slice: a client-server
//! transport over raw UDP, length-prefixed bincode messages, a
//! handshake, periodic transform snapshots from server to every
//! connected client, and a bidirectional "named event" channel that
//! both blueprints and C# code use as a generic RPC fabric.
//!
//! Authority model: server owns the truth. Each tick the server
//! collects entity transforms tagged with replication and broadcasts
//! them to every client. Clients apply incoming snapshots to their
//! local copy of the scene before running blueprints, so reads via
//! `get_position` / etc. observe replicated state.
//!
//! Reliability: snapshots are fire-and-forget UDP - acceptable for
//! state that's re-broadcast every tick. Named events use the same
//! transport; loss is the caller's problem to retry (a v2 ACK layer
//! is documented in the roadmap).
//!
//! Why no external deps: keeps the bootstrap surface small and
//! ports cleanly to mobile / WASM later by swapping the UDP
//! socket for a platform-appropriate transport. A future swap to
//! `renet` / `quinn` will preserve the message types verbatim.

#![forbid(unsafe_code)]

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Replication-eligible transform snapshot. Carries the entity's
/// stable scene id + 3D pose. 2D-only entities can use the same
/// shape with z = 0.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EntitySnapshot {
    pub entity_id: u32,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

/// Top-level wire message. One enum variant per traffic kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetMessage {
    /// Initial client -> server greeting with the protocol version
    /// the client speaks. Server rejects mismatches.
    Hello { proto: u32, name: String },
    /// Server -> client greeting back. Carries the client's freshly
    /// assigned numeric id so the client knows who it is.
    Welcome { proto: u32, client_id: u32 },
    /// Server -> client periodic snapshot of replicated entity
    /// transforms. Sent every server tick.
    Snapshot {
        tick: u64,
        entities: Vec<EntitySnapshot>,
    },
    /// Either direction: named event the application layer hooks
    /// (blueprints, C# RPC). Payload is a single float for now;
    /// richer marshalling is roadmap.
    Event { name: String, payload: f32 },
    /// Generic typed payload envelope. Applications (e.g. the voxel
    /// MP demo) bincode-encode their own structs and ship them
    /// through here, tagged by `kind` so the receiver can dispatch.
    /// Direction is bidirectional; servers can broadcast and clients
    /// can send to server. Loss is fire-and-forget UDP.
    Bytes { kind: String, data: Vec<u8> },
    /// Liveness probe; receiver replies with `Pong`.
    Ping,
    Pong,
}

/// Protocol number bumped on breaking message changes.
pub const PROTO_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum NetError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("encode error: {0}")]
    Encode(#[from] bincode::error::EncodeError),
    #[error("decode error: {0}")]
    Decode(#[from] bincode::error::DecodeError),
}

const MAX_PACKET: usize = 1500; // MTU-ish.

/// Server endpoint. Owns a non-blocking UDP socket; new clients are
/// recognised by their source address on the first packet received.
pub struct NetServer {
    socket: UdpSocket,
    clients: HashMap<SocketAddr, ClientSlot>,
    next_id: u32,
    tick: u64,
    inbox_events: Vec<(u32, String, f32)>,
    inbox_bytes: Vec<(u32, String, Vec<u8>)>,
}

#[derive(Debug, Clone)]
struct ClientSlot {
    id: u32,
    #[allow(dead_code)]
    name: String,
    last_seen: Instant,
}

impl std::fmt::Debug for NetServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetServer")
            .field("local_addr", &self.socket.local_addr().ok())
            .field("clients", &self.clients.len())
            .field("tick", &self.tick)
            .finish()
    }
}

impl NetServer {
    /// Bind a UDP socket on `addr` (e.g. `"0.0.0.0:7777"`). Returns
    /// the ready-to-poll server.
    pub fn bind(addr: &str) -> Result<Self, NetError> {
        let socket = UdpSocket::bind(addr)?;
        socket.set_nonblocking(true)?;
        tracing::info!(addr = %socket.local_addr()?, "net server bound");
        Ok(Self {
            socket,
            clients: HashMap::new(),
            next_id: 1,
            tick: 0,
            inbox_events: Vec::new(),
            inbox_bytes: Vec::new(),
        })
    }

    pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
        self.socket.local_addr()
    }

    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Drain incoming packets, update client state, accumulate any
    /// inbound events. Call once per server tick. Returns the number
    /// of packets processed.
    pub fn poll(&mut self) -> usize {
        let mut count = 0;
        let mut buf = [0u8; MAX_PACKET];
        loop {
            match self.socket.recv_from(&mut buf) {
                Ok((n, addr)) => {
                    count += 1;
                    match decode(&buf[..n]) {
                        Ok(msg) => self.handle_message(addr, msg),
                        Err(err) => tracing::warn!(error = %err, "server decode failed"),
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) => {
                    tracing::warn!(error = %e, "server recv error");
                    break;
                }
            }
        }
        // Reap clients that haven't been heard from in 5 seconds.
        let cutoff = Instant::now() - Duration::from_secs(5);
        self.clients.retain(|_, c| c.last_seen >= cutoff);
        count
    }

    fn handle_message(&mut self, addr: SocketAddr, msg: NetMessage) {
        match msg {
            NetMessage::Hello { proto, name } => {
                if proto != PROTO_VERSION {
                    tracing::warn!(addr = %addr, proto, "rejecting client with bad proto");
                    return;
                }
                let id = self.next_id;
                self.next_id = self.next_id.wrapping_add(1).max(1);
                self.clients.insert(
                    addr,
                    ClientSlot {
                        id,
                        name,
                        last_seen: Instant::now(),
                    },
                );
                let welcome = NetMessage::Welcome {
                    proto: PROTO_VERSION,
                    client_id: id,
                };
                let _ = self.send_to(&welcome, addr);
                tracing::info!(addr = %addr, id, "client joined");
            }
            NetMessage::Ping => {
                if let Some(c) = self.clients.get_mut(&addr) {
                    c.last_seen = Instant::now();
                }
                let _ = self.send_to(&NetMessage::Pong, addr);
            }
            NetMessage::Event { name, payload } => {
                if let Some(c) = self.clients.get(&addr) {
                    self.inbox_events.push((c.id, name, payload));
                }
            }
            NetMessage::Bytes { kind, data } => {
                if let Some(c) = self.clients.get(&addr) {
                    self.inbox_bytes.push((c.id, kind, data));
                }
            }
            _ => {}
        }
    }

    /// Broadcast a typed byte payload to every connected client.
    pub fn broadcast_bytes(&self, kind: &str, data: Vec<u8>) {
        let msg = NetMessage::Bytes {
            kind: kind.to_string(),
            data,
        };
        for addr in self.clients.keys() {
            let _ = self.send_to(&msg, *addr);
        }
    }

    /// Drain any typed payloads received from clients since the
    /// last call.
    pub fn drain_bytes(&mut self) -> Vec<(u32, String, Vec<u8>)> {
        std::mem::take(&mut self.inbox_bytes)
    }

    /// Broadcast a snapshot of every replicated entity to every
    /// connected client. Skip when no clients are connected.
    pub fn broadcast_snapshot(&mut self, entities: Vec<EntitySnapshot>) {
        self.tick = self.tick.wrapping_add(1);
        if self.clients.is_empty() || entities.is_empty() {
            return;
        }
        let msg = NetMessage::Snapshot {
            tick: self.tick,
            entities,
        };
        let addrs: Vec<SocketAddr> = self.clients.keys().copied().collect();
        for addr in addrs {
            let _ = self.send_to(&msg, addr);
        }
    }

    /// Send a named event to every client (e.g. a UI ping, score
    /// change). Mirrors the BP `net_send_event` node.
    pub fn broadcast_event(&self, name: &str, payload: f32) {
        let msg = NetMessage::Event {
            name: name.to_string(),
            payload,
        };
        for addr in self.clients.keys() {
            let _ = self.send_to(&msg, *addr);
        }
    }

    /// Drain any events received from clients since the last call.
    /// Returns `(client_id, event_name, payload)` triples.
    pub fn drain_events(&mut self) -> Vec<(u32, String, f32)> {
        std::mem::take(&mut self.inbox_events)
    }

    fn send_to(&self, msg: &NetMessage, addr: SocketAddr) -> Result<(), NetError> {
        let buf = encode(msg)?;
        self.socket.send_to(&buf, addr)?;
        Ok(())
    }
}

/// Client endpoint. Single UDP socket sending to a fixed server
/// address. Stores the most recent snapshot for the application
/// layer to apply each tick.
pub struct NetClient {
    socket: UdpSocket,
    server_addr: SocketAddr,
    handshake_done: bool,
    pub client_id: Option<u32>,
    pub last_snapshot: Option<(u64, Vec<EntitySnapshot>)>,
    inbox_events: Vec<(String, f32)>,
    inbox_bytes: Vec<(String, Vec<u8>)>,
    name: String,
    last_hello_sent: Option<Instant>,
}

impl std::fmt::Debug for NetClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NetClient")
            .field("server_addr", &self.server_addr)
            .field("client_id", &self.client_id)
            .field("handshake_done", &self.handshake_done)
            .finish()
    }
}

impl NetClient {
    /// Connect to `server_addr` (e.g. `"127.0.0.1:7777"`). The
    /// initial handshake fires from `poll`; check `handshake_done()`
    /// after polling to confirm the server replied.
    pub fn connect(server_addr: &str, name: impl Into<String>) -> Result<Self, NetError> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.set_nonblocking(true)?;
        let parsed: SocketAddr = server_addr
            .parse()
            .map_err(|e: std::net::AddrParseError| {
                NetError::Io(std::io::Error::new(ErrorKind::InvalidInput, e.to_string()))
            })?;
        Ok(Self {
            socket,
            server_addr: parsed,
            handshake_done: false,
            client_id: None,
            last_snapshot: None,
            inbox_events: Vec::new(),
            inbox_bytes: Vec::new(),
            name: name.into(),
            last_hello_sent: None,
        })
    }

    pub fn handshake_done(&self) -> bool {
        self.handshake_done
    }

    /// Drain incoming packets + re-send Hello while still
    /// handshaking. Call every client tick.
    pub fn poll(&mut self) -> usize {
        // Retry Hello at 0.5 Hz until the server replies.
        if !self.handshake_done {
            let should = self
                .last_hello_sent
                .is_none_or(|t| t.elapsed() > Duration::from_millis(500));
            if should {
                let hello = NetMessage::Hello {
                    proto: PROTO_VERSION,
                    name: self.name.clone(),
                };
                let _ = self.send(&hello);
                self.last_hello_sent = Some(Instant::now());
            }
        }
        let mut count = 0;
        let mut buf = [0u8; MAX_PACKET];
        loop {
            match self.socket.recv_from(&mut buf) {
                Ok((n, _from)) => {
                    count += 1;
                    match decode(&buf[..n]) {
                        Ok(msg) => self.handle_message(msg),
                        Err(err) => {
                            tracing::warn!(error = %err, "client decode failed")
                        }
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) => {
                    tracing::warn!(error = %e, "client recv error");
                    break;
                }
            }
        }
        count
    }

    fn handle_message(&mut self, msg: NetMessage) {
        match msg {
            NetMessage::Welcome { proto, client_id } => {
                if proto != PROTO_VERSION {
                    tracing::warn!(proto, "server proto mismatch");
                    return;
                }
                self.handshake_done = true;
                self.client_id = Some(client_id);
                tracing::info!(client_id, "client handshake complete");
            }
            NetMessage::Snapshot { tick, entities } => {
                self.last_snapshot = Some((tick, entities));
            }
            NetMessage::Event { name, payload } => {
                self.inbox_events.push((name, payload));
            }
            NetMessage::Bytes { kind, data } => {
                self.inbox_bytes.push((kind, data));
            }
            NetMessage::Pong => {}
            _ => {}
        }
    }

    pub fn send_event(&self, name: &str, payload: f32) -> Result<(), NetError> {
        self.send(&NetMessage::Event {
            name: name.to_string(),
            payload,
        })
    }

    pub fn send_bytes(&self, kind: &str, data: Vec<u8>) -> Result<(), NetError> {
        self.send(&NetMessage::Bytes {
            kind: kind.to_string(),
            data,
        })
    }

    pub fn drain_bytes(&mut self) -> Vec<(String, Vec<u8>)> {
        std::mem::take(&mut self.inbox_bytes)
    }

    pub fn ping(&self) -> Result<(), NetError> {
        self.send(&NetMessage::Ping)
    }

    pub fn drain_events(&mut self) -> Vec<(String, f32)> {
        std::mem::take(&mut self.inbox_events)
    }

    fn send(&self, msg: &NetMessage) -> Result<(), NetError> {
        let buf = encode(msg)?;
        self.socket.send_to(&buf, self.server_addr)?;
        Ok(())
    }
}

fn encode(msg: &NetMessage) -> Result<Vec<u8>, NetError> {
    let cfg = bincode::config::standard();
    let v = bincode::serde::encode_to_vec(msg, cfg)?;
    Ok(v)
}

fn decode(bytes: &[u8]) -> Result<NetMessage, NetError> {
    let cfg = bincode::config::standard();
    let (m, _) = bincode::serde::decode_from_slice(bytes, cfg)?;
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    /// End-to-end loopback: bind a server on an ephemeral port,
    /// connect a client, run both polls until the handshake
    /// completes, broadcast a snapshot, drain on the client side.
    #[test]
    fn loopback_handshake_and_snapshot() {
        let mut server = NetServer::bind("127.0.0.1:0").unwrap();
        let addr = server.local_addr().unwrap();
        let mut client = NetClient::connect(&addr.to_string(), "tester").unwrap();
        // Drive both for up to 2 seconds until the handshake clears.
        let start = Instant::now();
        while !client.handshake_done() && start.elapsed() < Duration::from_secs(2) {
            client.poll();
            server.poll();
            sleep(Duration::from_millis(20));
        }
        assert!(client.handshake_done());
        assert_eq!(server.client_count(), 1);
        // Broadcast snapshot + observe on the client.
        let snap = vec![EntitySnapshot {
            entity_id: 7,
            position: [1.0, 2.0, 3.0],
            rotation: [0.0; 3],
            scale: [1.0; 3],
        }];
        server.broadcast_snapshot(snap);
        // Pump the client until the snapshot arrives.
        let start = Instant::now();
        while client.last_snapshot.is_none() && start.elapsed() < Duration::from_secs(1) {
            client.poll();
            sleep(Duration::from_millis(20));
        }
        let (_, entities) = client.last_snapshot.clone().unwrap();
        assert_eq!(entities.len(), 1);
        assert_eq!(entities[0].entity_id, 7);
        assert_eq!(entities[0].position, [1.0, 2.0, 3.0]);
    }

    #[test]
    fn event_roundtrip() {
        let mut server = NetServer::bind("127.0.0.1:0").unwrap();
        let addr = server.local_addr().unwrap();
        let mut client = NetClient::connect(&addr.to_string(), "ev").unwrap();
        let start = Instant::now();
        while !client.handshake_done() && start.elapsed() < Duration::from_secs(2) {
            client.poll();
            server.poll();
            sleep(Duration::from_millis(20));
        }
        client.send_event("Shoot", 5.5).unwrap();
        let start = Instant::now();
        let mut got: Vec<(u32, String, f32)> = Vec::new();
        while got.is_empty() && start.elapsed() < Duration::from_secs(1) {
            server.poll();
            got.extend(server.drain_events());
            sleep(Duration::from_millis(20));
        }
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1, "Shoot");
        assert!((got[0].2 - 5.5).abs() < 1e-3);
    }
}
