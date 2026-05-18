//! Save / load orchestration for the host's authoritative world.
//!
//! Owns the on-disk read + write path that used to live as
//! `Voxel::save_server_state` / `Voxel::load_server_state` in
//! `main.rs`. The `Voxel` methods stay as thin wrappers so call
//! sites (autosave timer, shutdown hook, `/save` command) don't
//! have to know about the relocated implementation.
//!
//! Two layers live here:
//!
//! - `save` / `load`: the legacy single-file format. Reads /
//!   writes `voxel_server_state.bin` in the current working
//!   directory so existing players keep working across the A4
//!   refactor. Phase G (Worlds menu) is what flips this over to
//!   the per-slot path resolved through `layout`.
//!
//! - `layout`: cross-platform `worlds/<slug>/` resolver +
//!   `WorldMeta` schema. Wired up now so Phase B (dedicated
//!   server) and Phase G (UI) land against a stable disk shape.
//!
//! Wire types (`ServerStateV2`, `PersistedInv`) live in
//! `crate::net` because they share the bincode + serde dependency
//! tree with the rest of the protocol; we just import them.

use std::collections::HashMap;

use infengine_voxel::BlockId;
use tracing::{info, warn};

use crate::net::{
    EditMsg, NetRole, PersistedInv, SERVER_STATE_VERSION, ServerStateV2,
};
use crate::{DAY_LENGTH_S, PLAYER_MAX_HEALTH, Voxel, set_world_seed, world_seed};

pub(crate) mod layout;
pub(crate) mod meta;
pub(crate) mod world_json;

#[cfg(test)]
mod tests;

/// Path of the legacy server-side world delta file. Lives next to
/// the launch directory so a host can be torn down + brought back
/// up without losing the players' edits. The multi-world layout
/// (`layout::state_path(slug)`) replaces this in Phase G; the
/// legacy file is migrated into `worlds/legacy/` on first load.
pub(crate) const STATE_FILE: &str = "voxel_server_state.bin";

/// Magic bytes at the head of every save: `"INFV"` (InfEngine
/// Voxel). Distinguishes the new versioned format from the old
/// raw `Vec<EditMsg>` format - if the first 4 bytes aren't the
/// magic, the loader treats the whole file as a v1 edit log.
pub(crate) const STATE_MAGIC: &[u8; 4] = b"INFV";

/// Read the on-disk edit log and apply every entry to the local
/// world. Called once on the server right after the world's
/// procedural base is generated. Tries the versioned format
/// first, falls back to the legacy raw edit log on a magic
/// mismatch so existing save files keep working.
///
/// Also fires the one-shot migration from the pre-A4 single
/// global file (`voxel_server_state.bin` in CWD) to the new
/// `worlds/legacy/world/state.bin` slot. The migration is a move,
/// not a copy: after a successful relocate the legacy file is
/// gone. The runtime still reads + writes the CWD path for now;
/// Phase G is what actually drives the new slot.
pub(crate) fn load(voxel: &mut Voxel) {
    migrate_legacy_if_needed();

    let path = std::path::PathBuf::from(STATE_FILE);
    let Ok(bytes) = std::fs::read(&path) else {
        return;
    };
    let cfg = bincode::config::standard();
    // Path A: new versioned format with `INFV` magic.
    if bytes.len() >= 4 && &bytes[..4] == STATE_MAGIC {
        match bincode::serde::decode_from_slice::<ServerStateV2, _>(&bytes[4..], cfg) {
            Ok((state, _)) => {
                info!(
                    version = state.version,
                    edits = state.edits.len(),
                    clients = state.inventories.len(),
                    day_time = state.day_time,
                    path = %path.display(),
                    "loaded server state (v2)"
                );
                for e in &state.edits {
                    voxel.world_set_tracked([e.x, e.y, e.z], BlockId(e.block));
                }
                voxel.edit_history = state.edits;
                voxel.day_time = state.day_time.rem_euclid(DAY_LENGTH_S);
                voxel.player_health =
                    state.player_health.clamp(0.0, PLAYER_MAX_HEALTH);
                voxel.kill_count = state.kill_count;
                // Restore the world seed before the chunk
                // stream starts re-meshing so the workers
                // pick up the right noise. Older saves with
                // seed=0 stay at zero; on_start will fill in
                // a random value the first time the loader
                // doesn't catch.
                if state.world_seed != 0 {
                    set_world_seed(state.world_seed);
                }
                // Hydrate snow stack + torch orientation
                // overrides so a reloaded save matches the
                // exact state the host left.
                for (v, l) in state.snow_layers {
                    voxel.snow_positions.insert(v, l.clamp(1, 8));
                }
                for (v, o) in state.torch_orient {
                    voxel.torch_orient.insert(v, o);
                }
                // Replay the persisted water spread depths so
                // re-spread doesn't restart on every load. New
                // saves carry full depth info; older v2 saves
                // without the field land at depth 0 (treated
                // as source) which is fine for small pools.
                for (v, d) in state.liquid_level {
                    // Clamp at MC's 7-level range so a save
                    // from an old build with a smaller cap
                    // still loads cleanly into the new
                    // generic pipeline.
                    voxel.liquid_level.insert(v, d.min(7));
                }
                // Restore inventories into a side map indexed by
                // client name. They get re-keyed to live client
                // ids on connect (see `tick_net`).
                voxel.saved_inventories = state
                    .inventories
                    .into_iter()
                    .map(|(name, p)| (name, p.into_client_inv()))
                    .collect();
                voxel.rebuild_all_meshes();
                return;
            }
            Err(err) => {
                warn!(error = %err, path = %path.display(), "state load v2 decode failed");
                return;
            }
        }
    }
    // Path B: legacy `Vec<EditMsg>` format.
    let edits: Vec<EditMsg> = match bincode::serde::decode_from_slice(&bytes, cfg) {
        Ok((e, _)) => e,
        Err(err) => {
            warn!(error = %err, path = %path.display(), "state load (legacy) decode failed");
            return;
        }
    };
    info!(count = edits.len(), path = %path.display(), "loaded server state (legacy v1)");
    for e in &edits {
        voxel.world_set_tracked([e.x, e.y, e.z], BlockId(e.block));
    }
    voxel.edit_history = edits;
    voxel.rebuild_all_meshes();
}

/// Flush the in-memory edit log + per-client inventories +
/// day-of-cycle to disk in the new versioned `INFV` format.
/// Called periodically while the server runs (every 30 s) so a
/// crash loses at most half a minute of state.
pub(crate) fn save(voxel: &mut Voxel) {
    // Server hosts AND offline singleplayer can save.
    // Clients can't - the host owns the authoritative state.
    if !matches!(voxel.net, NetRole::Server { .. } | NetRole::Offline) {
        return;
    }
    let path = std::path::PathBuf::from(STATE_FILE);
    let cfg = bincode::config::standard();
    // Build a name -> PersistedInv map by joining live client
    // ids with their reported names (from NetServer) + the
    // saved_inventories side table (for clients that already
    // disconnected this run but still have state we want to
    // preserve across restarts).
    let mut by_name: HashMap<String, PersistedInv> = voxel
        .saved_inventories
        .iter()
        .map(|(n, inv)| (n.clone(), PersistedInv::from_client_inv(inv)))
        .collect();
    if let NetRole::Server {
        server,
        inventories,
        ..
    } = &voxel.net
    {
        for (cid, inv) in inventories.iter() {
            if let Some(name) = server.client_name(*cid) {
                by_name.insert(name, PersistedInv::from_client_inv(inv));
            }
        }
    }
    let state = ServerStateV2 {
        version: SERVER_STATE_VERSION,
        edits: voxel.edit_history.clone(),
        inventories: by_name.into_iter().collect(),
        day_time: voxel.day_time,
        player_health: voxel.player_health,
        kill_count: voxel.kill_count,
        world_seed: world_seed(),
        snow_layers: voxel
            .snow_positions
            .iter()
            .map(|(v, l)| (*v, *l))
            .collect(),
        torch_orient: voxel
            .torch_orient
            .iter()
            .map(|(v, o)| (*v, *o))
            .collect(),
        liquid_level: voxel
            .liquid_level
            .iter()
            .map(|(v, d)| (*v, *d))
            .collect(),
    };
    match bincode::serde::encode_to_vec(&state, cfg) {
        Ok(payload) => {
            let mut bytes = Vec::with_capacity(payload.len() + 4);
            bytes.extend_from_slice(STATE_MAGIC);
            bytes.extend_from_slice(&payload);
            if let Err(err) = std::fs::write(&path, &bytes) {
                warn!(error = %err, path = %path.display(), "state save failed");
            }
        }
        Err(err) => warn!(error = %err, "state save encode failed"),
    }
    voxel.last_state_save = Some(std::time::Instant::now());
    voxel.note_save();
}

/// One-shot migration: if the pre-A4 single global file
/// (`voxel_server_state.bin` in CWD) exists and the new
/// `worlds/legacy/world/state.bin` slot is empty, *copy* the
/// legacy bytes into the new slot. The runtime keeps reading +
/// writing the CWD path for now (Phase G flips the switch); this
/// step is purely about getting the bytes into the new shape so
/// the Worlds menu can later pick them up. The deliberate copy-
/// not-move keeps existing players' worlds loading at the CWD
/// path until Phase G is ready to retire it.
///
/// Idempotent: if the destination already exists OR the legacy
/// file is missing OR creating the destination dir fails, the
/// function is a no-op.
fn migrate_legacy_if_needed() {
    let legacy = std::path::PathBuf::from(STATE_FILE);
    if !legacy.exists() {
        return;
    }
    let dest = layout::state_path("legacy");
    if dest.exists() {
        return;
    }
    let Some(parent) = dest.parent() else {
        return;
    };
    if let Err(err) = std::fs::create_dir_all(parent) {
        warn!(
            error = %err,
            dir = %parent.display(),
            "world_save: legacy migration create_dir_all failed - skipping"
        );
        return;
    }
    match std::fs::copy(&legacy, &dest) {
        Ok(_) => {
            info!(
                from = %legacy.display(),
                to = %dest.display(),
                "world_save: migrated legacy save -> worlds/legacy/"
            );
        }
        Err(err) => {
            warn!(
                error = %err,
                from = %legacy.display(),
                to = %dest.display(),
                "world_save: legacy migration failed"
            );
        }
    }
}
