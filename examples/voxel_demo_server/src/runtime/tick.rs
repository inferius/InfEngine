//! Per-tick orchestration for the headless server.
//!
//! The headless tick is deliberately tiny in Phase B.7b:
//!
//! 1. `NetServer::poll` to drain incoming packets.
//! 2. Diff the live client roster against `known_clients` to emit
//!    audit-log connect / disconnect lines.
//! 3. Drain any incoming `vx.*` byte messages - currently logged at
//!    debug level; future commits hook them into a request handler.
//! 4. Drain incoming named events (RPC channel).
//!
//! World sim (liquid schedule, mob AI, drop physics, scheduled
//! timers) is deferred to the headless-parity follow-up that lifts
//! those subsystems out of `voxel_demo_mp::main::Voxel`. The server
//! today hosts a static world + acts as a roster broker.

use tracing::{debug, info};

use super::HeadlessServer;

/// Drive one tick. Called from `main`'s 20 Hz loop.
pub(super) fn run(srv: &mut HeadlessServer) {
    // 1. Drain net traffic.
    let _packets = srv.net_mut().poll();

    // 2. Diff the client roster.
    let live: std::collections::HashSet<u32> =
        srv.net_mut().client_ids().into_iter().collect();
    // New connections.
    let mut joined: Vec<u32> = live.difference(srv.known_clients_mut()).copied().collect();
    joined.sort();
    for id in joined {
        let name = srv
            .net_mut()
            .client_name(id)
            .unwrap_or_else(|| format!("client-{id}"));
        info!(id, %name, "client connected");
        srv.audit_mut().connect(id, &name, "unknown");
    }
    // Disconnects.
    let mut left: Vec<u32> = srv
        .known_clients_mut()
        .difference(&live)
        .copied()
        .collect();
    left.sort();
    for id in left {
        info!(id, "client disconnected");
        srv.audit_mut().disconnect(id, "unknown", "client_close");
    }
    *srv.known_clients_mut() = live;

    // 3. Drain incoming named events + byte messages. Phase B.7c
    // wires these into a request handler (chat dispatch / admin
    // commands); for now we just trace at debug level so an operator
    // running with `RUST_LOG=debug` sees the traffic shape.
    for (id, kind, _data) in srv.net_mut().drain_bytes() {
        debug!(client = id, %kind, "rx bytes message");
    }
    for (id, name, payload) in srv.net_mut().drain_events() {
        debug!(client = id, %name, payload, "rx event");
    }
}
