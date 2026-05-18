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

use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::atomic::{atomic_save, read_with_recovery};
use super::layout::{world_dir, world_dir_in, world_json_path, world_json_path_in};
use super::migrate::{self, MigrationRegistry, SaveKind};

/// Schema version baked into every fresh `world.json`. Bumped when
/// the struct fields change in a breaking way.
pub const WORLD_FORMAT_VERSION: u32 = 1;

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
}

/// Build a fresh `WorldJson` for a brand-new slot. Spawn defaults
/// to `[0, 0, 0]` - worldgen surface fix-up happens on first load.
pub fn create_world(worldgen: &str, seed: u32) -> WorldJson {
    WorldJson {
        format_version: WORLD_FORMAT_VERSION,
        worldgen: worldgen.to_string(),
        seed,
        spawn: [0, 0, 0],
        world_border_radius: DEFAULT_WORLD_BORDER_RADIUS,
        default_dimension: DEFAULT_DIMENSION.to_string(),
        dimensions: vec![DEFAULT_DIMENSION.to_string()],
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
        assert_eq!(w.dimensions, vec![DEFAULT_DIMENSION.to_string()]);
    }
}
