//! `world.json` - per-save worldgen + dimension configuration.
//!
//! See `docs/design/save_architecture_final.md` ("world.json") for
//! the canonical field list. Schema v1 fields:
//!
//! ```json
//! {
//!   "format_version": 1,
//!   "worldgen": "amplified",
//!   "seed": 17539214,
//!   "spawn": [12, 80, -8],
//!   "world_border_radius": 30000000,
//!   "default_dimension": "overworld",
//!   "dimensions": ["overworld", "nether"]
//! }
//! ```
//!
//! Separate from `meta.json` because the worldgen seed + dimension
//! list are read-only after creation - editing them mid-save would
//! invalidate every chunk on disk. The file lives next to `meta.json`
//! so the Worlds menu can show "Amplified, seed 17539214" without
//! deserialising any chunk data.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::atomic::{atomic_save, read_with_recovery};
use super::layout::{world_dir, world_dir_in, world_json_path, world_json_path_in};
use super::migrate::{self, MigrationRegistry, SaveKind};

/// Schema version baked into every fresh `world.json`. Bumped when
/// the struct fields change in a breaking way.
///
/// History:
/// - v1 (Phase B): initial layout (worldgen, seed, spawn, ...).
/// - v2 (Phase F): added `dimension_worldgens` (per-dim worldgen
///   map). v1 saves migrate by populating it from the global
///   `worldgen` field for every entry in `dimensions`.
/// - v3 (Wave 8.2): added `default_game_mode` (Survival / Creative
///   tag). v2 saves migrate by defaulting it to `"survival"`.
pub const WORLD_FORMAT_VERSION: u32 = 3;

/// Default game-mode tag baked into every fresh `world.json`. Matches
/// `GameMode::Survival::as_tag` - kept as a literal here so the
/// world_save module stays free of the `game_mode` import (the lib
/// crate re-mounts this file via `#[path]` and doesn't pull the bin's
/// `game_mode` module in).
pub(crate) const DEFAULT_GAME_MODE_TAG: &str = "survival";

/// Vanilla MC's world border default - 60 million blocks across,
/// 30 million radius from origin. Saves don't typically grow that
/// big but pinning the value here keeps it explicit.
pub(crate) const DEFAULT_WORLD_BORDER_RADIUS: u32 = 30_000_000;

/// Name of the dimension that the player spawns into when no
/// explicit dimension is selected. Phase F extends this with
/// per-dimension worldgen tables.
pub(crate) const DEFAULT_DIMENSION: &str = "overworld";

/// Per-save worldgen + dimension configuration. Immutable after
/// world creation - edits would invalidate every persisted chunk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorldJson {
    /// `WORLD_FORMAT_VERSION` at the time this slot was written.
    pub format_version: u32,
    /// Worldgen variant tag ("classic" / "flat" / "amplified" / ...).
    /// Read by the chunk-source pick on load.
    pub worldgen: String,
    /// Noise seed for the worldgen. 0 = the loader picks a fresh
    /// random value on first load.
    pub seed: u32,
    /// Initial player spawn in world coordinates. Defaults to
    /// `[0, 0, 0]` on creation; the worldgen surface code re-pins
    /// this to the surface on first start.
    pub spawn: [i32; 3],
    /// World border radius from origin, in blocks. See
    /// `DEFAULT_WORLD_BORDER_RADIUS`.
    pub world_border_radius: u32,
    /// Dimension the player spawns into when no explicit
    /// dimension is selected.
    pub default_dimension: String,
    /// All dimensions known to this save. Phase F populates this
    /// from a richer per-dim config table.
    pub dimensions: Vec<String>,
    /// Per-dimension worldgen variant lookup. Each key is a
    /// dimension name from `dimensions`, each value is the
    /// `worldgen` tag (`"classic"` / `"flat"` / `"nether"` / ...)
    /// used when generating that dim's chunks. New saves (Phase F)
    /// default to `{"overworld": <worldgen>, "nether": "nether"}`.
    /// Older saves migrate by copying the legacy `worldgen` field
    /// into the map for every known dim.
    #[serde(default)]
    pub dimension_worldgens: BTreeMap<String, String>,
    /// W8.2: default game mode for new joiners. Persisted as a free
    /// string tag so future GameMode variants land without touching
    /// the schema version. Recognised values: `"survival"`,
    /// `"creative"`. Unknown values round to Survival on load.
    /// v2 saves migrate by defaulting this field to `"survival"`.
    #[serde(default = "default_game_mode_tag")]
    pub default_game_mode: String,
}

/// `serde(default = ...)` hook for the `default_game_mode` field.
fn default_game_mode_tag() -> String {
    DEFAULT_GAME_MODE_TAG.to_string()
}

/// Name of the nether dimension. Phase F's second dimension.
pub(crate) const NETHER_DIMENSION: &str = "nether";

/// Worldgen tag used for the nether dim by default. Mirrors
/// `WorldGen::Nether::worldgen_str` so a round trip through the
/// on-disk JSON keeps producing nether terrain there.
pub(crate) const NETHER_WORLDGEN: &str = "nether";

/// Build a fresh `WorldJson` for a brand-new slot. Spawn defaults
/// to `[0, 0, 0]` - worldgen surface fix-up happens on first load.
///
/// Phase F: fresh slots now ship with two dimensions out of the
/// box - overworld (uses `worldgen`) and nether (uses the dedicated
/// `WorldGen::Nether` profile). The `dimension_worldgens` map keeps
/// the two assignments side by side so the per-dim chunk-stream
/// path doesn't have to guess.
pub fn create_world(worldgen: &str, seed: u32) -> WorldJson {
    create_world_with_mode(worldgen, seed, DEFAULT_GAME_MODE_TAG)
}

/// W8.2: variant of `create_world` that lets the caller pin a
/// non-default game mode (e.g. the user selected Creative on the
/// new-world screen). The mode is stored as a free string tag - see
/// `GameMode::as_tag` / `GameMode::from_tag` for the canonical
/// encoding.
pub fn create_world_with_mode(
    worldgen: &str,
    seed: u32,
    default_game_mode: &str,
) -> WorldJson {
    let mut dim_gens = BTreeMap::new();
    dim_gens.insert(DEFAULT_DIMENSION.to_string(), worldgen.to_string());
    dim_gens.insert(NETHER_DIMENSION.to_string(), NETHER_WORLDGEN.to_string());
    WorldJson {
        format_version: WORLD_FORMAT_VERSION,
        worldgen: worldgen.to_string(),
        seed,
        spawn: [0, 0, 0],
        world_border_radius: DEFAULT_WORLD_BORDER_RADIUS,
        default_dimension: DEFAULT_DIMENSION.to_string(),
        dimensions: vec![
            DEFAULT_DIMENSION.to_string(),
            NETHER_DIMENSION.to_string(),
        ],
        dimension_worldgens: dim_gens,
        default_game_mode: default_game_mode.to_string(),
    }
}

/// Read `<world_dir(slug)>/world.json`.
///
/// Refuses unknown `format_version` values unless the migration
/// registry has a chain to `WORLD_FORMAT_VERSION`. Hands off to
/// `migrate::default_registry`.
pub fn read_world(slug: &str) -> io::Result<WorldJson> {
    read_world_from_path(&world_json_path(slug), migrate::default_registry())
}

/// Test-friendly variant: read from an explicit base path.
pub fn read_world_in_dir(base: &Path, slug: &str) -> io::Result<WorldJson> {
    read_world_from_path(&world_json_path_in(base, slug), migrate::default_registry())
}

/// Same as `read_world_in_dir` but lets the caller inject a custom
/// migration registry - the canonical hook for migration unit tests.
pub fn read_world_in_dir_with_registry(
    base: &Path,
    slug: &str,
    registry: &MigrationRegistry,
) -> io::Result<WorldJson> {
    read_world_from_path(&world_json_path_in(base, slug), registry)
}

fn read_world_from_path(path: &Path, registry: &MigrationRegistry) -> io::Result<WorldJson> {
    let migrated_at_path = std::cell::Cell::new(false);
    let parse = |bytes: &[u8]| -> io::Result<WorldJson> {
        let (world, did_migrate) = decode_world(bytes, path, registry)?;
        if did_migrate {
            migrated_at_path.set(true);
        }
        Ok(world)
    };
    match read_with_recovery(path, parse)? {
        Some(world) => {
            if migrated_at_path.get() {
                let payload = encode_world(&world)?;
                atomic_save(path, &payload)?;
            }
            Ok(world)
        }
        None => Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("world.json missing at {}", path.display()),
        )),
    }
}

/// Parse + version-gate a `world.json` byte buffer. Free fn so the
/// primary read + the `.bak` recovery path share validation.
fn decode_world(
    bytes: &[u8],
    path: &Path,
    registry: &MigrationRegistry,
) -> io::Result<(WorldJson, bool)> {
    let disk_version = migrate::peek_json_version(bytes)?;
    let did_migrate = disk_version != WORLD_FORMAT_VERSION;
    let migrated = migrate::migrate_to(
        registry,
        SaveKind::WorldJson,
        WORLD_FORMAT_VERSION,
        path,
        disk_version,
        bytes,
    )?;
    let world: WorldJson = serde_json::from_slice(&migrated).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("world.json parse error: {err}"),
        )
    })?;
    if world.format_version != WORLD_FORMAT_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "world.json format_version {} not supported (expected {})",
                world.format_version, WORLD_FORMAT_VERSION
            ),
        ));
    }
    Ok((world, did_migrate))
}

/// Serialise a `WorldJson` into the same pretty-printed JSON used by
/// the writer. Used by the migration write-back path.
fn encode_world(world: &WorldJson) -> io::Result<Vec<u8>> {
    serde_json::to_vec_pretty(world).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("world.json encode error: {err}"),
        )
    })
}

/// Write `world.json` for a slot, creating the world directory if
/// missing. Atomic (tmp + rename) - see `meta::atomic_write`.
pub fn write_world(slug: &str, world: &WorldJson) -> io::Result<()> {
    let dir = world_dir(slug);
    let path = world_json_path(slug);
    write_world_to(&dir, &path, world)
}

/// Test-friendly variant: write to an explicit base path.
pub fn write_world_in_dir(base: &Path, slug: &str, world: &WorldJson) -> io::Result<()> {
    let dir = world_dir_in(base, slug);
    let path = world_json_path_in(base, slug);
    write_world_to(&dir, &path, world)
}

fn write_world_to(dir: &Path, path: &Path, world: &WorldJson) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let payload = serde_json::to_vec_pretty(world).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("world.json encode error: {err}"),
        )
    })?;
    atomic_save(path, &payload)
}

/// Migration arrow `world.json` v1 -> v2. Adds the
/// `dimension_worldgens` field by populating it from the legacy
/// global `worldgen` field for every entry in `dimensions` that
/// doesn't already have a per-dim assignment. A v1 save with
/// `dimensions: ["overworld"]` and `worldgen: "classic"` lifts to
/// v2 with `dimension_worldgens: {"overworld": "classic"}`.
pub(crate) fn world_json_v1_to_v2(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut value: serde_json::Value = serde_json::from_slice(bytes).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("world.json v1 parse failed: {err}"),
        )
    })?;
    let on_disk_v = value
        .get("format_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if on_disk_v != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("world.json v1 -> v2 expected v1, got v{on_disk_v}"),
        ));
    }
    let worldgen = value
        .get("worldgen")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("classic")
        .to_string();
    // Pull the dim list out so we can mint a map keyed by every dim.
    let dims_iter: Vec<String> = value
        .get("dimensions")
        .and_then(serde_json::Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|d| d.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_else(|| vec![DEFAULT_DIMENSION.to_string()]);
    let mut dim_map = serde_json::Map::new();
    for d in dims_iter {
        dim_map.insert(d, serde_json::Value::String(worldgen.clone()));
    }
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "dimension_worldgens".to_string(),
            serde_json::Value::Object(dim_map),
        );
        obj.insert("format_version".to_string(), serde_json::json!(2));
    }
    serde_json::to_vec_pretty(&value).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("world.json v2 encode failed: {err}"),
        )
    })
}

/// Migration arrow `world.json` v2 -> v3. Adds the
/// `default_game_mode` field with the literal `"survival"` value
/// so worlds created before Wave 8 land in Survival when reopened.
pub(crate) fn world_json_v2_to_v3(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut value: serde_json::Value = serde_json::from_slice(bytes).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("world.json v2 parse failed: {err}"),
        )
    })?;
    let on_disk_v = value
        .get("format_version")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    if on_disk_v != 2 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("world.json v2 -> v3 expected v2, got v{on_disk_v}"),
        ));
    }
    if let Some(obj) = value.as_object_mut() {
        obj.entry("default_game_mode").or_insert_with(|| {
            serde_json::Value::String(DEFAULT_GAME_MODE_TAG.to_string())
        });
        obj.insert("format_version".to_string(), serde_json::json!(3));
    }
    serde_json::to_vec_pretty(&value).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("world.json v3 encode failed: {err}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_world_uses_documented_defaults() {
        let w = create_world("classic", 42);
        assert_eq!(w.format_version, WORLD_FORMAT_VERSION);
        assert_eq!(w.worldgen, "classic");
        assert_eq!(w.seed, 42);
        assert_eq!(w.spawn, [0, 0, 0]);
        assert_eq!(w.world_border_radius, DEFAULT_WORLD_BORDER_RADIUS);
        assert_eq!(w.default_dimension, DEFAULT_DIMENSION);
        assert_eq!(
            w.dimensions,
            vec![
                DEFAULT_DIMENSION.to_string(),
                NETHER_DIMENSION.to_string(),
            ]
        );
        // Phase F: per-dim worldgen map is populated from
        // create_world directly. Overworld picks up the requested
        // generator; nether always uses the dedicated profile.
        assert_eq!(
            w.dimension_worldgens.get(DEFAULT_DIMENSION).map(String::as_str),
            Some("classic")
        );
        assert_eq!(
            w.dimension_worldgens.get(NETHER_DIMENSION).map(String::as_str),
            Some(NETHER_WORLDGEN)
        );
        // W8.2: fresh world defaults the game mode to Survival.
        assert_eq!(w.default_game_mode, DEFAULT_GAME_MODE_TAG);
    }

    #[test]
    fn create_world_with_mode_persists_creative() {
        // W8.2: when the user picks Creative on the new-world
        // screen, the WorldJson must store the tag verbatim.
        let w = create_world_with_mode("realistic", 7, "creative");
        assert_eq!(w.default_game_mode, "creative");
        assert_eq!(w.format_version, WORLD_FORMAT_VERSION);
    }

    #[test]
    fn world_json_round_trips_game_mode_through_serde() {
        // W8.2: serialise + deserialise a Creative world and
        // confirm the field survives unchanged.
        let w = create_world_with_mode("classic", 12345, "creative");
        let payload = serde_json::to_vec_pretty(&w).unwrap();
        let parsed: WorldJson = serde_json::from_slice(&payload).unwrap();
        assert_eq!(parsed.default_game_mode, "creative");
        assert_eq!(parsed.seed, 12345);
    }

    #[test]
    fn v2_to_v3_migration_defaults_game_mode_to_survival() {
        // W8.2: v2 saves lift cleanly by populating
        // `default_game_mode` with the literal "survival" tag.
        let v2 = serde_json::json!({
            "format_version": 2,
            "worldgen": "classic",
            "seed": 99,
            "spawn": [0, 0, 0],
            "world_border_radius": DEFAULT_WORLD_BORDER_RADIUS,
            "default_dimension": "overworld",
            "dimensions": ["overworld", "nether"],
            "dimension_worldgens": {"overworld": "classic", "nether": "nether"},
        });
        let bytes = serde_json::to_vec(&v2).unwrap();
        let migrated = world_json_v2_to_v3(&bytes).unwrap();
        let parsed: WorldJson = serde_json::from_slice(&migrated).unwrap();
        assert_eq!(parsed.format_version, 3);
        assert_eq!(parsed.default_game_mode, "survival");
    }

    #[test]
    fn v2_to_v3_migration_refuses_wrong_version() {
        let v3 = serde_json::json!({
            "format_version": 3,
            "worldgen": "classic",
            "seed": 0,
            "spawn": [0, 0, 0],
            "world_border_radius": DEFAULT_WORLD_BORDER_RADIUS,
            "default_dimension": "overworld",
            "dimensions": ["overworld"],
            "dimension_worldgens": {},
            "default_game_mode": "survival",
        });
        let bytes = serde_json::to_vec(&v3).unwrap();
        let res = world_json_v2_to_v3(&bytes);
        assert!(res.is_err(), "v3 input must be rejected as not-v2");
    }

    #[test]
    fn v1_to_v2_migration_populates_dim_worldgens() {
        // Build a v1-shaped payload (no dimension_worldgens, just
        // the global worldgen + the legacy single-entry dimensions
        // list) and run it through the migration arrow.
        let v1 = serde_json::json!({
            "format_version": 1,
            "worldgen": "amplified",
            "seed": 17,
            "spawn": [0, 0, 0],
            "world_border_radius": DEFAULT_WORLD_BORDER_RADIUS,
            "default_dimension": "overworld",
            "dimensions": ["overworld"],
        });
        let bytes = serde_json::to_vec(&v1).unwrap();
        let migrated = world_json_v1_to_v2(&bytes).unwrap();
        let parsed: WorldJson = serde_json::from_slice(&migrated).unwrap();
        assert_eq!(parsed.format_version, 2);
        assert_eq!(parsed.dimensions, vec!["overworld".to_string()]);
        assert_eq!(
            parsed.dimension_worldgens.get("overworld").map(String::as_str),
            Some("amplified")
        );
        // The migration does NOT auto-add nether to a v1 save - the
        // user has to opt in by recreating the world.
        assert!(!parsed.dimension_worldgens.contains_key("nether"));
    }

    #[test]
    fn v1_to_v2_migration_refuses_wrong_version() {
        let v2 = serde_json::json!({
            "format_version": 2,
            "worldgen": "classic",
            "seed": 0,
            "spawn": [0, 0, 0],
            "world_border_radius": DEFAULT_WORLD_BORDER_RADIUS,
            "default_dimension": "overworld",
            "dimensions": ["overworld"],
            "dimension_worldgens": {},
        });
        let bytes = serde_json::to_vec(&v2).unwrap();
        let res = world_json_v1_to_v2(&bytes);
        assert!(res.is_err(), "v2 input must be rejected as not-v1");
    }
}
