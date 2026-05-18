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

use super::layout::{world_dir, world_dir_in, world_json_path, world_json_path_in};
use super::meta::atomic_write;

/// Schema version baked into every fresh `world.json`. Bumped when
/// the struct fields change in a breaking way.
pub(crate) const WORLD_FORMAT_VERSION: u32 = 1;

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
pub(crate) struct WorldJson {
    /// `WORLD_FORMAT_VERSION` at the time this slot was written.
    pub(crate) format_version: u32,
    /// Worldgen variant tag ("classic" / "flat" / "amplified" / ...).
    /// Read by the chunk-source pick on load.
    pub(crate) worldgen: String,
    /// Noise seed for the worldgen. 0 = the loader picks a fresh
    /// random value on first load.
    pub(crate) seed: u32,
    /// Initial player spawn in world coordinates. Defaults to
    /// `[0, 0, 0]` on creation; the worldgen surface code re-pins
    /// this to the surface on first start.
    pub(crate) spawn: [i32; 3],
    /// World border radius from origin, in blocks. See
    /// `DEFAULT_WORLD_BORDER_RADIUS`.
    pub(crate) world_border_radius: u32,
    /// Dimension the player spawns into when no explicit
    /// dimension is selected.
    pub(crate) default_dimension: String,
    /// All dimensions known to this save. Phase F populates this
    /// from a richer per-dim config table.
    pub(crate) dimensions: Vec<String>,
}

/// Build a fresh `WorldJson` for a brand-new slot. Spawn defaults
/// to `[0, 0, 0]` - worldgen surface fix-up happens on first load.
pub(crate) fn create_world(worldgen: &str, seed: u32) -> WorldJson {
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
/// Refuses unknown `format_version` values with an
/// `io::ErrorKind::InvalidData`.
pub(crate) fn read_world(slug: &str) -> io::Result<WorldJson> {
    read_world_from_path(&world_json_path(slug))
}

/// Test-friendly variant: read from an explicit base path.
pub(crate) fn read_world_in_dir(base: &Path, slug: &str) -> io::Result<WorldJson> {
    read_world_from_path(&world_json_path_in(base, slug))
}

fn read_world_from_path(path: &Path) -> io::Result<WorldJson> {
    let bytes = std::fs::read(path)?;
    let world: WorldJson = serde_json::from_slice(&bytes).map_err(|err| {
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
    Ok(world)
}

/// Write `world.json` for a slot, creating the world directory if
/// missing. Atomic (tmp + rename) - see `meta::atomic_write`.
pub(crate) fn write_world(slug: &str, world: &WorldJson) -> io::Result<()> {
    let dir = world_dir(slug);
    let path = world_json_path(slug);
    write_world_to(&dir, &path, world)
}

/// Test-friendly variant: write to an explicit base path.
pub(crate) fn write_world_in_dir(
    base: &Path,
    slug: &str,
    world: &WorldJson,
) -> io::Result<()> {
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
    atomic_write(path, &payload)
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
