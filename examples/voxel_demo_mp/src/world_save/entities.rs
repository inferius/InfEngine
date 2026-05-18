//! Per-dimension entity persistence: mobs + dropped items.
//!
//! See `docs/design/save_architecture_final.md` ("entities/<dim>.bin")
//! for the canonical field list. Wire summary:
//!
//! ```text
//! [u8;  4]   magic = b"INFE"
//! [u32]      format_version  (little-endian)
//! [u8]       compression_kind (0 = none, 1 = zstd)
//! [u64]      uncompressed_payload_len (little-endian)
//! [bytes]    payload (bincode-serialised DimensionEntities, optionally
//!            zstd-compressed)
//! ```
//!
//! Same envelope shape as `chunks.rs` + `players.rs` - one decoder per
//! file family. One file per dimension (not per chunk) because mobs
//! roam across chunks; rebalancing per-chunk entity bins on every
//! cross-boundary step would thrash. A bulk dimension file rewrites
//! once per autosave instead.
//!
//! Block + mob ids are stored as raw `u16` / `u8` on the wire + disk:
//! keeps the schema free of the `infengine_voxel` crate's type
//! registry so a server-side ID renumber would not blow up existing
//! entity files. The caller maps `u16 <-> BlockId` and `u8 <-> MobKind`
//! at the edge of the world-save layer.
//!
//! Phase B.7 is what wires this into the live save / load path. Right
//! now the module is exercised purely by the unit tests in
//! `world_save/entities_tests.rs`.
#![allow(dead_code)]

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::atomic::{atomic_save, read_with_recovery};
use super::layout::{world_dir, world_dir_in};
use super::migrate::{self, MigrationRegistry, SaveKind};

/// Magic bytes at the head of every entities file: `"INFE"` (InfEngine
/// entities). Distinguishes the format from chunks (`INFC`), players
/// (`INFP`), and timers (`INFT`).
pub(crate) const ENTITIES_MAGIC: &[u8; 4] = b"INFE";

/// Envelope + payload format version. Bumped when the on-disk shape
/// changes in a breaking way. Phase B.6 wires a migration registry.
pub(crate) const ENTITIES_FORMAT_VERSION: u32 = 1;

/// Default zstd compression level. Matches `chunks.rs` + `players.rs` -
/// level 3 is the documented "balanced" setting from the design doc.
const ZSTD_LEVEL: i32 = 3;

/// Compression-kind discriminant byte stored in the envelope.
const COMP_NONE: u8 = 0;
const COMP_ZSTD: u8 = 1;

/// All persistent entity state for one dimension.
///
/// One file per dimension (e.g. `entities/overworld.bin`). Live and
/// dead mobs aren't distinguished here - the loader is expected to
/// drop entries with health <= 0 on the way in if it doesn't want
/// them re-spawned.
///
/// Block ids in `DropSave` are stored as raw `u16` so the schema does
/// not pull in `infengine_voxel`; mob kind is `u8` for the same
/// reason. The caller maps back to the live type at the edge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct DimensionEntities {
    /// Payload-level format version. Matches the envelope version
    /// today (both bump together); Phase B.6 may split them.
    pub format_version: u32,
    /// Every persistent mob in the dimension.
    pub mobs: Vec<MobSave>,
    /// Every dropped item still on the ground.
    pub drops: Vec<DropSave>,
    /// Monotonic id source for the next minted mob. Stored so a
    /// reload doesn't recycle an id that was previously alive and
    /// might still be referenced elsewhere in the save (e.g. by a
    /// leashed-to-fence relationship in the future).
    pub next_mob_id: u32,
    /// Same as `next_mob_id` but for dropped items.
    pub next_drop_id: u32,
}

impl DimensionEntities {
    /// Build a fresh empty `DimensionEntities` at the current format
    /// version. Useful as the initial value when a dimension is first
    /// touched + as the baseline for tests.
    pub(crate) fn empty() -> Self {
        Self {
            format_version: ENTITIES_FORMAT_VERSION,
            mobs: Vec::new(),
            drops: Vec::new(),
            next_mob_id: 0,
            next_drop_id: 0,
        }
    }
}

/// One persistent mob entry.
///
/// `kind` is the runtime's `MobKind` discriminator stored as raw `u8`
/// (KIND_SHEEP / KIND_ZOMBIE / ...). Keeping the on-disk schema free
/// of the enum type lets the runtime renumber variants without
/// invalidating existing saves - the caller maps `u8 <-> MobKind` at
/// the edge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct MobSave {
    /// Stable per-save mob id. Allocated from
    /// `DimensionEntities::next_mob_id`.
    pub id: u32,
    /// Mob kind discriminator (KIND_SHEEP / KIND_ZOMBIE / ...). Raw
    /// `u8` so a `MobKind` renumber doesn't break existing saves.
    pub kind: u8,
    /// World-space position `[x, y, z]`.
    pub pos: [f32; 3],
    /// Heading - rotation around Y, radians.
    pub yaw: f32,
    /// World-space velocity `[vx, vy, vz]`.
    pub vel: [f32; 3],
    /// Current health.
    pub health: f32,
    /// Age in seconds since spawn. Used by behaviours that gate on
    /// lifetime (e.g. baby -> adult transitions).
    pub age_seconds: f32,
    // Future fields go here. New optional fields land via
    // `#[serde(default)]` so old saves still load cleanly.
}

/// One persistent dropped-item entry. Despawn timers live on the
/// runtime side; here we just store the age so the dispatcher can
/// re-derive remaining time on reload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct DropSave {
    /// Stable per-save drop id. Allocated from
    /// `DimensionEntities::next_drop_id`.
    pub id: u32,
    /// Block id stored as raw `u16` so the schema does not pull in
    /// `infengine_voxel`. Caller maps to `BlockId` at the edge.
    pub block: u16,
    /// World-space position `[x, y, z]`.
    pub pos: [f32; 3],
    /// World-space velocity `[vx, vy, vz]`.
    pub vel: [f32; 3],
    /// Age in seconds since drop. The runtime applies its own
    /// despawn cap (typically 5 min) against this on the load side.
    pub age: f32,
}

/// Canonical path of an entities file:
/// `<world_dir>/entities/<dim>.bin`.
pub(crate) fn entities_path(slug: &str, dim: &str) -> PathBuf {
    entities_path_under(&world_dir(slug), dim)
}

/// Same as `entities_path` but rooted at an explicit base path. Used
/// by tests + the future hosted-server tooling that drives saves out
/// of a non-default directory.
pub(crate) fn entities_path_in_dir(base: &Path, slug: &str, dim: &str) -> PathBuf {
    entities_path_under(&world_dir_in(base, slug), dim)
}

fn entities_path_under(world: &Path, dim: &str) -> PathBuf {
    world.join("entities").join(format!("{dim}.bin"))
}

/// Encode + write the dimension's entity bundle to its canonical slot.
/// Creates the `entities/` directory if missing.
pub(crate) fn write_entities(
    slug: &str,
    dim: &str,
    entities: &DimensionEntities,
) -> io::Result<()> {
    write_entities_to(&entities_path(slug, dim), entities)
}

/// Test-friendly write variant rooted at an explicit base path.
pub(crate) fn write_entities_in_dir(
    base: &Path,
    slug: &str,
    dim: &str,
    entities: &DimensionEntities,
) -> io::Result<()> {
    write_entities_to(&entities_path_in_dir(base, slug, dim), entities)
}

/// Read + decode an entities file. Returns `Ok(None)` if the file
/// does not exist - the server-boot path treats a missing dim file
/// as "no entities to spawn yet" (fresh world, no mobs persisted)
/// and must not surface that as an error.
pub(crate) fn read_entities(slug: &str, dim: &str) -> io::Result<Option<DimensionEntities>> {
    read_entities_from(&entities_path(slug, dim), migrate::default_registry())
}

/// Test-friendly read variant rooted at an explicit base path.
pub(crate) fn read_entities_in_dir(
    base: &Path,
    slug: &str,
    dim: &str,
) -> io::Result<Option<DimensionEntities>> {
    read_entities_from(
        &entities_path_in_dir(base, slug, dim),
        migrate::default_registry(),
    )
}

/// Same as `read_entities_in_dir` but lets the caller inject a custom
/// migration registry - the canonical hook for migration unit tests.
pub(crate) fn read_entities_in_dir_with_registry(
    base: &Path,
    slug: &str,
    dim: &str,
    registry: &MigrationRegistry,
) -> io::Result<Option<DimensionEntities>> {
    read_entities_from(&entities_path_in_dir(base, slug, dim), registry)
}

fn write_entities_to(path: &Path, entities: &DimensionEntities) -> io::Result<()> {
    let cfg = bincode::config::standard();
    let raw = bincode::serde::encode_to_vec(entities, cfg)
        .map_err(|err| invalid(format!("entities payload encode failed: {err}")))?;
    let uncompressed_len = raw.len() as u64;

    // Same "pick smaller" policy as `chunks.rs` / `players.rs`: a
    // populated dimension with many mobs compresses well via zstd; a
    // near-empty file is so small the zstd overhead exceeds it.
    let (kind, body): (u8, Vec<u8>) = match zstd::encode_all(&raw[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw),
    };

    let mut bytes: Vec<u8> = Vec::with_capacity(body.len() + 17);
    bytes.extend_from_slice(ENTITIES_MAGIC);
    bytes.extend_from_slice(&ENTITIES_FORMAT_VERSION.to_le_bytes());
    bytes.push(kind);
    bytes.extend_from_slice(&uncompressed_len.to_le_bytes());
    bytes.extend_from_slice(&body);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    atomic_save(path, &bytes)
}

fn read_entities_from(
    path: &Path,
    registry: &MigrationRegistry,
) -> io::Result<Option<DimensionEntities>> {
    let migrated_at_path = std::cell::Cell::new(false);
    let migrated_bytes_holder: std::cell::RefCell<Option<Vec<u8>>> = std::cell::RefCell::new(None);
    let parse = |bytes: &[u8]| -> io::Result<DimensionEntities> {
        let (payload, did_migrate, new_bytes) = decode_entities_bytes(bytes, path, registry)?;
        if did_migrate {
            migrated_at_path.set(true);
            *migrated_bytes_holder.borrow_mut() = Some(new_bytes);
        }
        Ok(payload)
    };
    let opt = read_with_recovery(path, parse)?;
    if migrated_at_path.get()
        && let Some(bytes) = migrated_bytes_holder.into_inner()
    {
        atomic_save(path, &bytes)?;
    }
    Ok(opt)
}

/// Parse the on-disk envelope + bincode payload. Free fn so both the
/// primary read and the `.bak` recovery path inside
/// `atomic::read_with_recovery` run the exact same validation.
///
/// Returns `(payload, did_migrate, post_migration_bytes)`. See
/// `chunks::decode_chunk_bytes` for the rationale on returning the
/// post-migration bytes so the outer caller can write them back.
fn decode_entities_bytes(
    bytes: &[u8],
    path: &Path,
    registry: &MigrationRegistry,
) -> io::Result<(DimensionEntities, bool, Vec<u8>)> {
    if bytes.len() < 17 {
        return Err(invalid("entities file truncated header"));
    }
    if &bytes[..4] != ENTITIES_MAGIC {
        return Err(invalid("entities file magic mismatch"));
    }
    let disk_version = migrate::peek_envelope_version(bytes)?;
    let did_migrate = disk_version != ENTITIES_FORMAT_VERSION;
    let migrated = migrate::migrate_to(
        registry,
        SaveKind::Entities,
        ENTITIES_FORMAT_VERSION,
        path,
        disk_version,
        bytes,
    )?;
    let post = migrated.as_slice();
    if post.len() < 17 {
        return Err(invalid("entities file truncated header (post-migration)"));
    }
    if &post[..4] != ENTITIES_MAGIC {
        return Err(invalid("entities file magic mismatch (post-migration)"));
    }
    let version = u32::from_le_bytes([post[4], post[5], post[6], post[7]]);
    if version != ENTITIES_FORMAT_VERSION {
        return Err(invalid(format!(
            "entities format_version {version} not supported (expected {ENTITIES_FORMAT_VERSION})"
        )));
    }
    let kind = post[8];
    let uncompressed_len = u64::from_le_bytes([
        post[9], post[10], post[11], post[12], post[13], post[14], post[15], post[16],
    ]) as usize;
    let body = &post[17..];

    let raw: Vec<u8> = match kind {
        COMP_NONE => {
            if body.len() != uncompressed_len {
                return Err(invalid("uncompressed entities body length mismatch"));
            }
            body.to_vec()
        }
        COMP_ZSTD => decode_zstd(body, uncompressed_len)?,
        other => {
            return Err(invalid(format!(
                "unknown entities compression kind {other}"
            )));
        }
    };

    let cfg = bincode::config::standard();
    let (entities, _read) = bincode::serde::decode_from_slice::<DimensionEntities, _>(&raw, cfg)
        .map_err(|err| invalid(format!("entities payload decode failed: {err}")))?;
    Ok((entities, did_migrate, migrated))
}

/// Decompress `body` via zstd into a buffer of exactly `expected_len`
/// bytes. Mirrors the chunks.rs / players.rs helpers - bounds the
/// output so a maliciously inflated header can't trigger an unbounded
/// allocation.
fn decode_zstd(body: &[u8], expected_len: usize) -> io::Result<Vec<u8>> {
    let mut decoder = zstd::stream::Decoder::new(body)?;
    let mut out: Vec<u8> = Vec::with_capacity(expected_len);
    let mut buf = [0u8; 4096];
    loop {
        let n = decoder.read(&mut buf)?;
        if n == 0 {
            break;
        }
        if out.len() + n > expected_len {
            return Err(invalid("zstd decode exceeded uncompressed_payload_len"));
        }
        out.write_all(&buf[..n])?;
    }
    if out.len() != expected_len {
        return Err(invalid(
            "zstd decode produced fewer bytes than uncompressed_payload_len",
        ));
    }
    Ok(out)
}

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_constructor_uses_current_format_version() {
        let e = DimensionEntities::empty();
        assert_eq!(e.format_version, ENTITIES_FORMAT_VERSION);
        assert!(e.mobs.is_empty());
        assert!(e.drops.is_empty());
        assert_eq!(e.next_mob_id, 0);
        assert_eq!(e.next_drop_id, 0);
    }
}
