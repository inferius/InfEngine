//! Per-dimension block-state persistence: chest contents today,
//! future per-position state (furnaces, signs, jukeboxes) will
//! join the same file family without bumping the schema for
//! existing chests.
//!
//! See `docs/design/save_architecture_final.md` ("Per-block state")
//! for the canonical contract. Wire summary:
//!
//! ```text
//! [u8;  4]   magic = b"INFB"
//! [u32]      format_version  (little-endian)
//! [u8]       compression_kind (0 = none, 1 = zstd)
//! [u64]      uncompressed_payload_len (little-endian)
//! [bytes]    payload (bincode-serialised DimensionContainers,
//!            optionally zstd-compressed)
//! ```
//!
//! Same envelope shape as `entities.rs` + `chunks.rs` + `players.rs` -
//! one decoder per file family. One file per dimension (not per
//! chunk) because chests stay put but a player's chunk-rebuild
//! pattern would force per-chunk container splits otherwise.
//! A bulk dimension file rewrites once per autosave.
//!
//! Item ids inside a `ChestSave` slot are stored as raw `u16` so
//! the schema does not pull in `infengine_voxel`'s type registry -
//! a server-side ID renumber would not blow up existing container
//! files. The caller maps `u16 <-> BlockId` at the edge.
#![allow(dead_code)]

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::atomic::{atomic_save, read_with_recovery};
use super::layout::{world_dir, world_dir_in};
use super::migrate::{self, MigrationRegistry, SaveKind};

/// Magic bytes at the head of every containers file: `"INFB"`
/// (InfEngine Block-state). Distinguishes the format from chunks
/// (`INFC`), entities (`INFE`), players (`INFP`), timers (`INFT`).
pub(crate) const CONTAINERS_MAGIC: &[u8; 4] = b"INFB";

/// Envelope + payload format version. Bumped when the on-disk shape
/// changes in a breaking way. v1 = chest-slots only.
pub(crate) const CONTAINERS_FORMAT_VERSION: u32 = 1;

/// Default zstd compression level. Matches the sibling save modules.
const ZSTD_LEVEL: i32 = 3;

/// Compression-kind discriminant byte stored in the envelope.
const COMP_NONE: u8 = 0;
const COMP_ZSTD: u8 = 1;

/// All persistent block-state for one dimension.
///
/// One file per dimension (e.g. `containers/overworld.bin`). Only
/// chests today; future per-position state lands as new fields
/// alongside `chests` (and gets a `format_version` bump if the
/// addition isn't a `#[serde(default)]` compatible extension).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct DimensionContainers {
    /// Payload-level format version. Matches the envelope version
    /// today (both bump together).
    pub format_version: u32,
    /// Every chest with non-default state in the dimension. An
    /// empty chest still gets persisted so a freshly-placed empty
    /// chest survives a round trip (the world block alone doesn't
    /// know the slot count it should mint).
    pub chests: Vec<ChestSave>,
}

impl DimensionContainers {
    /// Build a fresh empty bundle at the current format version.
    pub(crate) fn empty() -> Self {
        Self {
            format_version: CONTAINERS_FORMAT_VERSION,
            chests: Vec::new(),
        }
    }
}

/// One persistent chest entry.
///
/// `slots` is the chest's slot vec, in slot-index order. Each
/// entry is `Some((item_id, count))` or `None` for empty. The
/// length is recorded per-chest so a future double-chest variant
/// can ship with 54 slots without touching the schema for existing
/// 27-slot saves.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ChestSave {
    /// Absolute world voxel coords of the chest block.
    pub pos: [i32; 3],
    /// Slot contents in slot-index order.
    pub slots: Vec<Option<(u16, u8)>>,
}

/// Canonical path of a containers file:
/// `<world_dir>/containers/<dim>.bin`.
pub(crate) fn containers_path(slug: &str, dim: &str) -> PathBuf {
    containers_path_under(&world_dir(slug), dim)
}

/// Same as `containers_path` but rooted at an explicit base path.
/// Used by tests + tooling that drives saves out of a non-default
/// directory.
pub(crate) fn containers_path_in_dir(base: &Path, slug: &str, dim: &str) -> PathBuf {
    containers_path_under(&world_dir_in(base, slug), dim)
}

fn containers_path_under(world: &Path, dim: &str) -> PathBuf {
    world.join("containers").join(format!("{dim}.bin"))
}

/// Encode + write the dimension's container bundle to its canonical
/// slot. Creates the `containers/` directory if missing.
pub(crate) fn write_containers(
    slug: &str,
    dim: &str,
    containers: &DimensionContainers,
) -> io::Result<()> {
    write_containers_to(&containers_path(slug, dim), containers)
}

/// Test-friendly write variant rooted at an explicit base path.
pub(crate) fn write_containers_in_dir(
    base: &Path,
    slug: &str,
    dim: &str,
    containers: &DimensionContainers,
) -> io::Result<()> {
    write_containers_to(&containers_path_in_dir(base, slug, dim), containers)
}

/// Read + decode a containers file. Returns `Ok(None)` if the file
/// does not exist - the server-boot path treats a missing dim file
/// as "no chests persisted yet" (fresh world) and must not surface
/// that as an error.
pub(crate) fn read_containers(slug: &str, dim: &str) -> io::Result<Option<DimensionContainers>> {
    read_containers_from(&containers_path(slug, dim), migrate::default_registry())
}

/// Test-friendly read variant rooted at an explicit base path.
pub(crate) fn read_containers_in_dir(
    base: &Path,
    slug: &str,
    dim: &str,
) -> io::Result<Option<DimensionContainers>> {
    read_containers_from(
        &containers_path_in_dir(base, slug, dim),
        migrate::default_registry(),
    )
}

/// Same as `read_containers_in_dir` but lets the caller inject a
/// custom migration registry - the canonical hook for migration
/// unit tests.
pub(crate) fn read_containers_in_dir_with_registry(
    base: &Path,
    slug: &str,
    dim: &str,
    registry: &MigrationRegistry,
) -> io::Result<Option<DimensionContainers>> {
    read_containers_from(&containers_path_in_dir(base, slug, dim), registry)
}

fn write_containers_to(path: &Path, containers: &DimensionContainers) -> io::Result<()> {
    let cfg = bincode::config::standard();
    let raw = bincode::serde::encode_to_vec(containers, cfg)
        .map_err(|err| invalid(format!("containers payload encode failed: {err}")))?;
    let uncompressed_len = raw.len() as u64;

    // Same "pick smaller" policy as the sibling save modules: a
    // populated dimension with many similar slot vecs compresses
    // well via zstd; a near-empty file is so small the zstd
    // overhead exceeds it.
    let (kind, body): (u8, Vec<u8>) = match zstd::encode_all(&raw[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw),
    };

    let mut bytes: Vec<u8> = Vec::with_capacity(body.len() + 17);
    bytes.extend_from_slice(CONTAINERS_MAGIC);
    bytes.extend_from_slice(&CONTAINERS_FORMAT_VERSION.to_le_bytes());
    bytes.push(kind);
    bytes.extend_from_slice(&uncompressed_len.to_le_bytes());
    bytes.extend_from_slice(&body);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    atomic_save(path, &bytes)
}

fn read_containers_from(
    path: &Path,
    registry: &MigrationRegistry,
) -> io::Result<Option<DimensionContainers>> {
    let migrated_at_path = std::cell::Cell::new(false);
    let migrated_bytes_holder: std::cell::RefCell<Option<Vec<u8>>> = std::cell::RefCell::new(None);
    let parse = |bytes: &[u8]| -> io::Result<DimensionContainers> {
        let (payload, did_migrate, new_bytes) = decode_containers_bytes(bytes, path, registry)?;
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

/// Parse the on-disk envelope + bincode payload. Free fn so both
/// the primary read and the `.bak` recovery path inside
/// `atomic::read_with_recovery` run the exact same validation.
///
/// Returns `(payload, did_migrate, post_migration_bytes)`.
fn decode_containers_bytes(
    bytes: &[u8],
    path: &Path,
    registry: &MigrationRegistry,
) -> io::Result<(DimensionContainers, bool, Vec<u8>)> {
    if bytes.len() < 17 {
        return Err(invalid("containers file truncated header"));
    }
    if &bytes[..4] != CONTAINERS_MAGIC {
        return Err(invalid("containers file magic mismatch"));
    }
    let disk_version = migrate::peek_envelope_version(bytes)?;
    let did_migrate = disk_version != CONTAINERS_FORMAT_VERSION;
    let migrated = migrate::migrate_to(
        registry,
        SaveKind::Containers,
        CONTAINERS_FORMAT_VERSION,
        path,
        disk_version,
        bytes,
    )?;
    let post = migrated.as_slice();
    if post.len() < 17 {
        return Err(invalid("containers file truncated header (post-migration)"));
    }
    if &post[..4] != CONTAINERS_MAGIC {
        return Err(invalid("containers file magic mismatch (post-migration)"));
    }
    let version = u32::from_le_bytes([post[4], post[5], post[6], post[7]]);
    if version != CONTAINERS_FORMAT_VERSION {
        return Err(invalid(format!(
            "containers format_version {version} not supported (expected {CONTAINERS_FORMAT_VERSION})"
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
                return Err(invalid("uncompressed containers body length mismatch"));
            }
            body.to_vec()
        }
        COMP_ZSTD => decode_zstd(body, uncompressed_len)?,
        other => {
            return Err(invalid(format!(
                "unknown containers compression kind {other}"
            )));
        }
    };

    let cfg = bincode::config::standard();
    let (containers, _read) =
        bincode::serde::decode_from_slice::<DimensionContainers, _>(&raw, cfg)
            .map_err(|err| invalid(format!("containers payload decode failed: {err}")))?;
    Ok((containers, did_migrate, migrated))
}

/// Decompress `body` via zstd into a buffer of exactly `expected_len`
/// bytes. Mirrors the sibling save modules' helpers - bounds the
/// output so a maliciously inflated header can't trigger an
/// unbounded allocation.
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
        let c = DimensionContainers::empty();
        assert_eq!(c.format_version, CONTAINERS_FORMAT_VERSION);
        assert!(c.chests.is_empty());
    }
}
