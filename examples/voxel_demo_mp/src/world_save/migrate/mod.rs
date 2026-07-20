//! Format-version migration framework.
//!
//! Every persisted struct under `world_save` carries a
//! `format_version: u32` field (either as the first serde field in a
//! bincode payload, or as a top-level JSON key). When a reader picks
//! up a file whose disk version doesn't match the binary's current
//! version, this module:
//!
//! 1. Looks up a chain of registered migration steps
//!    `from_v -> from_v + 1 -> ... -> target_v` for the relevant
//!    `SaveKind`.
//! 2. Runs each step on the raw file bytes in order.
//! 3. Returns the migrated bytes so the caller can decode + (in B.7+)
//!    write back at the new version via `atomic_save`.
//!
//! If no chain reaches the current version, the caller refuses the
//! load + writes a `.error` sidecar (see `refuse_with_error_sidecar`)
//! so a human can inspect the mismatch.
//!
//! ## State of the world
//!
//! The `default_registry` starts EMPTY - no production migration
//! arrows yet because every payload type is at `format_version = 1`.
//! Future bumps add `register(...)` calls inside their own modules'
//! init paths. The framework lives now so the next bump only needs to
//! wire its arrow, not invent the plumbing.
//!
//! Per `file_size_policy.md`: we start with a single `mod.rs`. Once
//! the first real migration arrow lands, promote into per-arrow files
//! like `chunk_v1_to_v2.rs`.
//!
//! See `docs/design/save_architecture_final.md`
//! ("Format versioning + migration") for the canonical contract.
#![allow(dead_code)]

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;

/// One-step migration. Takes the on-disk bytes of the from-version
/// payload + returns the on-disk bytes of the to-version payload.
///
/// The framework treats payloads as opaque byte buffers: a chunk
/// envelope's bytes are different from a `meta.json` blob's bytes,
/// but the step signature is the same. Each step knows which format
/// family it operates on via the `SaveKind` it's registered under.
pub(crate) type MigrationStep = fn(&[u8]) -> io::Result<Vec<u8>>;

/// Identifier for what kind of save file we're migrating. Lets the
/// registry keep separate chains per file kind so a `WorldMeta` v1->v2
/// arrow doesn't accidentally fire on a `ChunkPayload` v1 file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SaveKind {
    /// `meta.json` - JSON header.
    Meta,
    /// `world.json` - JSON worldgen config.
    WorldJson,
    /// `chunks/<dim>/<cx>_<cy>_<cz>.chunk.bin` - bincode + zstd
    /// envelope around `ChunkPayload`.
    Chunk,
    /// `players/<name>.bin` - bincode + zstd envelope around
    /// `PlayerSave`.
    Player,
    /// `entities/<dim>.bin` - bincode + zstd envelope around
    /// `DimensionEntities`.
    Entities,
    /// `timers/scheduled.bin` - bincode + zstd envelope around
    /// `ScheduledTimers`.
    Timers,
    /// `containers/<dim>.bin` - bincode + zstd envelope around
    /// `DimensionContainers`. Per-position block-state save for
    /// chests (and future furnaces / signs / ...).
    Containers,
}

/// Pure registry. The only mutating call is `register` at startup.
///
/// Keys are `(kind, from_version)`. Value is `(to_version, step)`.
/// Each `(kind, from)` slot holds exactly one outgoing edge so the
/// chain is unambiguous - register fires a debug-assert if two arrows
/// would compete for the same `(kind, from)` pair.
#[derive(Default)]
pub struct MigrationRegistry {
    steps: HashMap<(SaveKind, u32), (u32, MigrationStep)>,
}

// Manual `Debug` impl: `MigrationStep` is a `fn` pointer and the
// derive macro emits a name expression that doesn't compile against
// the workspace `missing_debug_implementations = warn` rule when the
// struct goes `pub`. Reporting just the count + the entry keys is
// enough for diagnostic dumps; the step pointer itself isn't useful
// to print.
impl std::fmt::Debug for MigrationRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let keys: Vec<_> = self.steps.keys().collect();
        f.debug_struct("MigrationRegistry")
            .field("registered", &keys)
            .finish()
    }
}

impl MigrationRegistry {
    /// Build a fresh empty registry. Tests use this to keep the
    /// `default_registry` singleton out of their state.
    pub(crate) fn new() -> Self {
        Self {
            steps: HashMap::new(),
        }
    }

    /// Register a one-step migration from `from` to `to` for `kind`.
    ///
    /// Panics in debug builds if two arrows compete for the same
    /// `(kind, from)` slot - that would make `plan` ambiguous. Release
    /// builds let the later registration win and log a warning.
    pub(crate) fn register(&mut self, kind: SaveKind, from: u32, to: u32, step: MigrationStep) {
        debug_assert!(from < to, "migration step must go forward: {from} -> {to}");
        let prev = self.steps.insert((kind, from), (to, step));
        if prev.is_some() {
            tracing::warn!(
                ?kind,
                from,
                to,
                "migrate: duplicate registration; latest wins"
            );
        }
    }

    /// Find a chain of steps from `from` to `target`. Returns
    /// `Some(steps)` if reachable, `None` if not.
    ///
    /// Greedy: at each version, takes the registered outgoing edge.
    /// Direct skips (e.g. v1 -> v3) are supported as long as `to`
    /// lands on or below `target`. A step that overshoots `target`
    /// is treated as no chain.
    pub(crate) fn plan(
        &self,
        kind: SaveKind,
        from: u32,
        target: u32,
    ) -> Option<Vec<MigrationStep>> {
        if from == target {
            return Some(Vec::new());
        }
        if from > target {
            // Downgrades are not supported - the design doc only
            // covers forward migration.
            return None;
        }
        let mut chain: Vec<MigrationStep> = Vec::new();
        let mut cur = from;
        // Cap iteration to prevent a malformed registry cycle from
        // hanging the load path. `target - from` is a hard ceiling on
        // legitimate chains anyway.
        let max_steps = (target - from) as usize + 1;
        for _ in 0..max_steps {
            let (to, step) = *self.steps.get(&(kind, cur))?;
            if to > target {
                return None;
            }
            chain.push(step);
            cur = to;
            if cur == target {
                return Some(chain);
            }
        }
        None
    }

    /// Apply the chain and return the migrated bytes. Returns the
    /// input unchanged when `from == target`.
    pub(crate) fn apply(
        &self,
        kind: SaveKind,
        from: u32,
        target: u32,
        payload: &[u8],
    ) -> io::Result<Vec<u8>> {
        let chain = self.plan(kind, from, target).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("migrate: no chain registered for {kind:?} {from} -> {target}"),
            )
        })?;
        let mut bytes = payload.to_vec();
        for step in chain {
            bytes = step(&bytes)?;
        }
        Ok(bytes)
    }
}

/// Process-wide default registry. Lazily initialised on first read.
///
/// Production migration arrows register here from
/// `register_production_arrows`. Each save kind's migration sits in
/// its own helper to keep this dispatcher short.
pub fn default_registry() -> &'static MigrationRegistry {
    static REGISTRY: OnceLock<MigrationRegistry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut reg = MigrationRegistry::new();
        register_production_arrows(&mut reg);
        reg
    })
}

/// Append the production migration arrows to `reg`. Currently:
/// - `Containers` v1 -> v2: adds an empty `furnaces` vec to the
///   payload (the new C.4 field).
/// - `WorldJson` v1 -> v2: lifts the legacy single `worldgen` field
///   into a per-dimension map.
/// - `Player` v1 -> v2: adds `highlight_mode: u8` (Q.2 targeting
///   wireframe mode); v1 saves migrate to `0` (= `Auto`).
///
/// Per the design doc, the right place for save-kind-specific
/// migrations is alongside the on-disk struct definitions. We pull
/// each step from its owning module so the migration code stays
/// co-located with the schema.
fn register_production_arrows(reg: &mut MigrationRegistry) {
    reg.register(
        SaveKind::Containers,
        1,
        2,
        super::containers::containers_v1_to_v2,
    );
    // Q.4f: v2 -> v3 adds the `fires` vec for the new fire-block
    // age state. v2 saves migrate to a v3 with `fires: vec![]`
    // so any pre-Wave-2 world still loads cleanly + persists at v3
    // on the next save.
    reg.register(
        SaveKind::Containers,
        2,
        3,
        super::containers::containers_v2_to_v3,
    );
    // W137: entities v1 -> v2 adds `carts` + `next_cart_id`
    // (minecart persistence). v1 saves migrate with empty carts.
    reg.register(
        SaveKind::Entities,
        1,
        2,
        super::entities::entities_v1_to_v2,
    );
    // W302: entities v2 -> v3 adds the structure-mob anchor set +
    // per-mob persistence/baby fields (villages stay populated).
    reg.register(
        SaveKind::Entities,
        2,
        3,
        super::entities::entities_v2_to_v3,
    );
    // W348: entities v3 -> v4 adds per-mob tamed/temper (horse
    // taming survives reload).
    reg.register(
        SaveKind::Entities,
        3,
        4,
        super::entities::entities_v3_to_v4,
    );
    // W138: containers v3 -> v4 adds doors / chest facings /
    // brewing stands / levers-on (the runtime block-state maps
    // that were never persisted - reloaded doors rendered open-
    // reversed and stayed impassable).
    reg.register(
        SaveKind::Containers,
        3,
        4,
        super::containers::containers_v3_to_v4,
    );
    // W143: containers v4 -> v5 adds stored rail shapes (stateful
    // MC RailState port). v4 saves migrate with `rails: vec![]`;
    // legacy rails backfill from the old derivation on first
    // remesh, freezing their current look.
    reg.register(
        SaveKind::Containers,
        4,
        5,
        super::containers::containers_v4_to_v5,
    );
    // world.json v1 -> v2: add per-dim worldgen map. v1 saves
    // (Phase B/C/D/E) lift cleanly by copying the legacy
    // `worldgen` field into the new `dimension_worldgens` map for
    // every entry in `dimensions`.
    reg.register(
        SaveKind::WorldJson,
        1,
        2,
        super::world_json::world_json_v1_to_v2,
    );
    // W8.2: world.json v2 -> v3 adds the `default_game_mode` tag
    // (Survival / Creative). Old saves migrate to "survival".
    reg.register(
        SaveKind::WorldJson,
        2,
        3,
        super::world_json::world_json_v2_to_v3,
    );
    reg.register(
        SaveKind::Player,
        1,
        2,
        super::players::player_v1_to_v2,
    );
    // W8.2: player.bin v2 -> v3 appends `game_mode: u8`. Old
    // saves migrate to `0` (= Survival), mirroring the v1 -> v2
    // arrow's strategy for the highlight_mode field.
    reg.register(
        SaveKind::Player,
        2,
        3,
        super::players::player_v2_to_v3,
    );
}

/// JSON peek struct - decodes just the `format_version` key and
/// ignores everything else. Works for `meta.json` + `world.json` which
/// both put the field at the top level of their object.
#[derive(Debug, Clone, Deserialize)]
struct JsonVersionShell {
    format_version: u32,
}

/// Peek a `format_version` out of a JSON byte buffer. Used by the
/// `meta.json` + `world.json` readers before we decide whether to
/// migrate.
pub(crate) fn peek_json_version(bytes: &[u8]) -> io::Result<u32> {
    let shell: JsonVersionShell = serde_json::from_slice(bytes).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("migrate: peek_json_version parse failed: {err}"),
        )
    })?;
    Ok(shell.format_version)
}

/// Peek a `format_version` out of a binary envelope file (chunks,
/// players, entities, timers). The shared envelope shape is:
///
/// ```text
/// [u8; 4]  magic
/// [u32]    format_version  (little-endian)
/// ...
/// ```
///
/// So the version sits at bytes 4..8. Refuses files shorter than 8
/// bytes - that can never be a valid envelope.
pub(crate) fn peek_envelope_version(bytes: &[u8]) -> io::Result<u32> {
    if bytes.len() < 8 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "migrate: envelope too short to peek version",
        ));
    }
    Ok(u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]))
}

/// Drive the migration for a file once the caller has already peeked
/// the disk version. If the disk version matches the target the input
/// is passed through; otherwise the registry chain runs, or - if no
/// chain reaches the target - the caller's load is refused with a
/// `.error.<ts>` sidecar.
pub(crate) fn migrate_to(
    registry: &MigrationRegistry,
    kind: SaveKind,
    target: u32,
    path: &Path,
    disk: u32,
    bytes: &[u8],
) -> io::Result<Vec<u8>> {
    if disk == target {
        return Ok(bytes.to_vec());
    }
    match registry.plan(kind, disk, target) {
        Some(_) => registry.apply(kind, disk, target, bytes),
        None => Err(refuse_with_error_sidecar(path, kind, disk, target)?),
    }
}

/// Write the `.error` sidecar + return the corresponding `io::Error`
/// the caller bubbles up to its own caller.
///
/// Sidecar path uses a timestamp suffix (matching `atomic::broken_path`'s
/// idea) so two version-mismatch attempts inside the same second don't
/// stomp each other's diagnostics.
fn refuse_with_error_sidecar(
    path: &Path,
    kind: SaveKind,
    disk: u32,
    target: u32,
) -> io::Result<io::Error> {
    let err_path = error_sidecar_path(path);
    let msg = format!(
        "save format_version {disk} not supported (current {target}). \
         No migration chain registered for {kind:?}. Refusing load.\n\
         File: {}\n",
        path.display()
    );
    // Best-effort: even if the sidecar write fails (read-only FS,
    // missing parent dir), still return the version-mismatch error
    // to the caller so they can refuse cleanly.
    if let Some(parent) = err_path.parent()
        && !parent.as_os_str().is_empty()
    {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&err_path, msg.as_bytes());
    Ok(io::Error::other(format!(
        "format_version {disk} unsupported; see {}",
        err_path.display()
    )))
}

/// `<path>.error.YYYYMMDD-HHMMSS-mmm` - the diagnostics landing pad
/// for a version mismatch. UTC ms-precision suffix so retries inside
/// the same second don't collide.
fn error_sidecar_path(path: &Path) -> PathBuf {
    let now = chrono::Utc::now();
    let stamp = now.format("%Y%m%d-%H%M%S-%3f").to_string();
    let mut s: std::ffi::OsString = path.as_os_str().to_owned();
    s.push(".error.");
    s.push(stamp);
    PathBuf::from(s)
}

#[cfg(test)]
mod registry_tests;
