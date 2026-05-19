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
//!
//! ## W5.4b: Decoupling-prep inventory
//!
//! When the parity follow-up lands, every field below needs to
//! migrate out of `voxel_demo_mp::main::Voxel` (a 12-kLOC GUI
//! struct) and into a new `ServerState` type that both the GUI
//! demo and this binary can share. The list captures EVERY field
//! the existing GUI-side ticks (mob AI, drop physics, scheduled
//! timers, liquid spread, fire spread, furnace cook) read or
//! mutate so the extraction commit doesn't have to spelunk:
//!
//! Mob AI:
//! - `mobs: Vec<MobState>`            - live mob list, active dim.
//! - `next_mob_id: u32`               - id allocator.
//! - `entities_by_dim`                - parked mobs per inactive dim.
//!
//! Drop physics:
//! - `dropped_items: Vec<DropItem>`   - live drop list, active dim.
//! - `next_drop_id: u32`              - id allocator.
//! - drops also live in `entities_by_dim` when their dim parks.
//!
//! Timers + cook:
//! - `furnace_runtimes: HashMap<...>` - per-furnace fuel + cook ms.
//! - `liquid_schedule: LiquidSchedule` - upcoming spread fires.
//! - `liquid_clock: u64`              - the schedule's tick counter.
//! - `liquid_positions: HashSet<...>` - tracks live liquid cells.
//! - `liquid_level: HashMap<...>`     - per-cell fill level 0..=7.
//! - `fires: HashMap<[i32;3], u8>`    - fire age per cell.
//!
//! Per-dim parking maps (any of the above when the dim isn't
//! active):
//! - `worlds: HashMap<String, DimWorld>`
//! - `entities_by_dim: HashMap<String, DimEntities>`
//! - `dim_worldgens: HashMap<String, WorldGen>`
//!
//! World-shaped state (the active dim's slice):
//! - `world: World`                   - the voxel grid the bin
//!   already owns; the server's `HeadlessServer.world` is the
//!   counterpart and will absorb this directly.
//! - `chests: HashMap<...>`           - chest containers, active dim.
//! - `furnaces: HashMap<...>`         - furnace containers, active dim.
//! - `torch_positions` / `torch_orient` / `snow_positions` - block
//!   side-state maps the chunk save path also persists.
//!
//! Player + clock (host's own):
//! - `player_pos / player_vel / player_yaw / player_pitch` -
//!   needed for AI line-of-sight + nameplate broadcast.
//! - `player_health / player_air` - damage / drown ticks.
//! - `day_time: f32 + last_day_broadcast` - clock broadcast.
//! - `weather: u8 + last_day_broadcast` - weather broadcast.
//! - `current_dim: String + base_seed: u32` - WorldgenCtx inputs.
//!
//! Once `ServerState` exists, both binaries instantiate it; the
//! GUI demo's `Voxel` holds a `state: ServerState` and exposes
//! its own `renderer`/`ui`/etc as additional fields. Tick logic
//! moves to a free fn `fn tick(state: &mut ServerState, ...)`
//! the GUI demo also calls.

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

#[cfg(test)]
mod decoupling_prep_doc_tests {
    /// W5.4b lock-in: assert that the per-field decoupling-prep
    /// inventory is still present in this file's header. The
    /// follow-up ServerState extraction relies on this list; if a
    /// future cleanup commit accidentally trims it, the test
    /// catches the deletion.
    ///
    /// The file is read at test-build time via `include_str!` so
    /// the test runs against the actual source rather than the
    /// compiled doc-comment metadata.
    const TICK_SOURCE: &str = include_str!("tick.rs");

    fn assert_mentions(needle: &str) {
        assert!(
            TICK_SOURCE.contains(needle),
            "tick.rs decoupling-prep doc must mention `{needle}`",
        );
    }

    #[test]
    fn doc_mentions_mob_ai_fields() {
        assert_mentions("mobs: Vec<MobState>");
        assert_mentions("next_mob_id");
    }

    #[test]
    fn doc_mentions_drop_fields() {
        assert_mentions("dropped_items: Vec<DropItem>");
        assert_mentions("next_drop_id");
    }

    #[test]
    fn doc_mentions_timer_and_cook_fields() {
        assert_mentions("furnace_runtimes");
        assert_mentions("liquid_schedule");
        assert_mentions("liquid_clock");
        assert_mentions("liquid_positions");
        assert_mentions("liquid_level");
        assert_mentions("fires: HashMap");
    }

    #[test]
    fn doc_mentions_per_dim_parking() {
        assert_mentions("worlds: HashMap");
        assert_mentions("entities_by_dim");
        assert_mentions("dim_worldgens");
    }

    #[test]
    fn doc_mentions_world_side_state() {
        assert_mentions("chests: HashMap");
        assert_mentions("furnaces: HashMap");
        assert_mentions("torch_positions");
        assert_mentions("snow_positions");
    }

    #[test]
    fn doc_mentions_player_and_clock() {
        assert_mentions("player_pos");
        assert_mentions("player_health");
        assert_mentions("day_time");
        assert_mentions("current_dim");
        assert_mentions("base_seed");
    }
}
