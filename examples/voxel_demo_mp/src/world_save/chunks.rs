//! Per-chunk persistent format: RLE-compressed voxels + bincode +
//! zstd envelope.
//!
//! See `docs/design/save_architecture_final.md`
//! ("chunks/<dim>/cx_cy_cz.chunk.bin") for the canonical byte
//! layout. Wire summary:
//!
//! ```text
//! [u8;  4]   magic = b"INFC"
//! [u32]      format_version  (little-endian)
//! [u8]       compression_kind (0 = none, 1 = zstd)
//! [u64]      uncompressed_payload_len (little-endian)
//! [bytes]    payload (bincode-serialised ChunkPayload, optionally
//!            zstd-compressed)
//! ```
//!
//! The envelope `format_version` (this file's `CHUNK_FORMAT_VERSION`)
//! tracks the on-disk shell layout. A separate `format_version`
//! field lives INSIDE the bincode payload (`ChunkPayload::
//! format_version`) so that once we add new payload fields in B.6
//! the inner version can advance independently of the outer one.
//!
//! Compression policy: encode raw bincode, then run zstd. If the
//! compressed bytes are smaller than the raw bytes, write
//! `kind = 1`; otherwise fall back to `kind = 0`. Uniform-air
//! chunks compress to ~10 bytes; tiny chunks where zstd overhead
//! would exceed the payload skip the compressor entirely.
//!
//! Phase B.7 is what wires this into the live save / load path.
//! Right now the module is exercised purely by the unit tests in
//! `world_save/tests.rs`.
#![allow(dead_code)]

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use infengine_voxel::{BlockId, CHUNK_SIZE, CHUNK_VOXELS, Chunk};
use serde::{Deserialize, Serialize};

use super::atomic::{atomic_save, read_with_recovery};
use super::layout::{world_dir, world_dir_in};
use super::migrate::{self, MigrationRegistry, SaveKind};

/// Magic bytes at the head of every chunk file: `"INFC"` (InfEngine
/// chunk). Distinguishes the new per-chunk format from anything
/// else the user might drop into the chunks dir.
pub(crate) const CHUNK_MAGIC: &[u8; 4] = b"INFC";

/// Envelope version (on-disk shell). Bumped when the byte-level
/// layout above changes; payload-only changes bump
/// `ChunkPayload::format_version` instead.
pub(crate) const CHUNK_FORMAT_VERSION: u32 = 1;

/// Default zstd compression level. Level 3 is the documented
/// "balanced" setting from the design doc - low CPU cost on writes,
/// near-optimal ratio on uniform-content chunks.
const ZSTD_LEVEL: i32 = 3;

/// Compression-kind discriminant byte stored in the envelope.
const COMP_NONE: u8 = 0;
const COMP_ZSTD: u8 = 1;

/// Run-length encoded voxel grid for one cubic chunk.
///
/// Each `(block_id, count)` covers `count` consecutive cells in
/// row-major `[x, y, z]` order starting from index 0. The sum of
/// `count`s across all runs is always `CHUNK_VOXELS` (4096); a
/// decoded chunk must consume exactly that many cells.
///
/// A uniformly-air chunk encodes as one run `(0, 4096)`. A
/// chequerboard pattern encodes as ~4096 single-cell runs (the
/// worst case for RLE), but bincode + zstd still keeps the file
/// well under the raw `Vec<BlockId>` size thanks to the back-end
/// compressor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ChunkVoxels {
    pub runs: Vec<(u16, u16)>,
}

impl ChunkVoxels {
    /// Encode a `Chunk` into RLE runs over its raw `[BlockId; 4096]`
    /// backing store.
    pub fn from_chunk(chunk: &Chunk) -> Self {
        let data = chunk.raw();
        let mut runs: Vec<(u16, u16)> = Vec::new();
        let mut iter = data.iter();
        let Some(first) = iter.next() else {
            return Self { runs };
        };
        let mut cur = first.0;
        let mut count: u16 = 1;
        for b in iter {
            if b.0 == cur && count < u16::MAX {
                count += 1;
            } else {
                runs.push((cur, count));
                cur = b.0;
                count = 1;
            }
        }
        runs.push((cur, count));
        Self { runs }
    }

    /// Decode back into a fresh `Chunk`. Returns an
    /// `io::ErrorKind::InvalidData` if the run counts don't sum to
    /// exactly `CHUNK_VOXELS` - a malformed payload must not silently
    /// truncate or overflow the chunk buffer.
    pub fn into_chunk(self) -> io::Result<Chunk> {
        let mut chunk = Chunk::empty();
        let mut idx: usize = 0;
        for (id, count) in self.runs {
            let end = idx
                .checked_add(count as usize)
                .ok_or_else(|| invalid("RLE run count overflow"))?;
            if end > CHUNK_VOXELS {
                return Err(invalid("RLE run count exceeds CHUNK_VOXELS"));
            }
            // Air runs leave the empty chunk buffer untouched - a
            // shortcut on the common uniformly-air case.
            if id != 0 {
                for i in idx..end {
                    let (x, y, z) = unflatten(i);
                    chunk.set(x, y, z, BlockId(id));
                }
            }
            idx = end;
        }
        if idx != CHUNK_VOXELS {
            return Err(invalid("RLE run counts do not sum to CHUNK_VOXELS"));
        }
        Ok(chunk)
    }
}

/// Bincode-encoded payload that lives inside the envelope.
///
/// Mirrors the wire format the server will send to clients in Phase
/// B's `vx.chunk_snapshot` message - one struct, two codecs (disk
/// adds zstd on top, wire keeps raw bincode).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ChunkPayload {
    /// Payload-level format version. Independent of the envelope
    /// version so we can add fields here without rewriting every
    /// chunk file's outer header.
    pub format_version: u32,
    /// World-space chunk coordinate this payload describes. Stored
    /// inside the file as well as in the filename so a stray
    /// `mv` doesn't desync the two.
    pub coord: [i32; 3],
    /// RLE voxel grid.
    pub voxels: ChunkVoxels,
    /// Per-chunk liquid spread depth overrides. Coordinate is
    /// chunk-local `[x, y, z]` (0..CHUNK_SIZE).
    pub liquid_levels: Vec<([u8; 3], u8)>,
    /// Per-chunk torch orientation overrides. Same coord scheme.
    pub torch_orient: Vec<([u8; 3], u8)>,
    /// Per-chunk snow stack height overrides. Same coord scheme.
    pub snow_layers: Vec<([u8; 3], u8)>,
}

/// Payload-version constant. See `CHUNK_FORMAT_VERSION` for the
/// rationale on the two-version split.
pub(crate) const CHUNK_PAYLOAD_VERSION: u32 = 1;

/// Path to one chunk file:
/// `<world_dir>/chunks/<dim>/<cx>_<cy>_<cz>.chunk.bin`.
pub(crate) fn chunk_path(slug: &str, dim: &str, coord: [i32; 3]) -> PathBuf {
    chunk_path_under(&world_dir(slug), dim, coord)
}

/// Same as `chunk_path` but rooted at an explicit base path. Used
/// by tests + the future hosted-server tooling that drives saves
/// out of a non-default directory.
pub(crate) fn chunk_path_in_dir(base: &Path, slug: &str, dim: &str, coord: [i32; 3]) -> PathBuf {
    chunk_path_under(&world_dir_in(base, slug), dim, coord)
}

fn chunk_path_under(world: &Path, dim: &str, coord: [i32; 3]) -> PathBuf {
    world
        .join("chunks")
        .join(dim)
        .join(format!("{}_{}_{}.chunk.bin", coord[0], coord[1], coord[2]))
}

/// Encode + write a chunk payload to its canonical slot.
pub(crate) fn write_chunk(slug: &str, dim: &str, payload: &ChunkPayload) -> io::Result<()> {
    write_chunk_to(&chunk_path(slug, dim, payload.coord), payload)
}

/// Test-friendly write variant rooted at an explicit base path.
pub(crate) fn write_chunk_in_dir(
    base: &Path,
    slug: &str,
    dim: &str,
    payload: &ChunkPayload,
) -> io::Result<()> {
    write_chunk_to(&chunk_path_in_dir(base, slug, dim, payload.coord), payload)
}

/// Read + decode a chunk payload. Returns `Ok(None)` if the file
/// does not exist - the streaming layer treats a missing chunk as
/// "regenerate from worldgen" and that path must not be an error.
pub(crate) fn read_chunk(
    slug: &str,
    dim: &str,
    coord: [i32; 3],
) -> io::Result<Option<ChunkPayload>> {
    read_chunk_from(&chunk_path(slug, dim, coord), migrate::default_registry())
}

/// Test-friendly read variant rooted at an explicit base path.
pub(crate) fn read_chunk_in_dir(
    base: &Path,
    slug: &str,
    dim: &str,
    coord: [i32; 3],
) -> io::Result<Option<ChunkPayload>> {
    read_chunk_from(
        &chunk_path_in_dir(base, slug, dim, coord),
        migrate::default_registry(),
    )
}

/// Same as `read_chunk_in_dir` but lets the caller inject a custom
/// migration registry - the canonical hook for migration unit tests.
pub(crate) fn read_chunk_in_dir_with_registry(
    base: &Path,
    slug: &str,
    dim: &str,
    coord: [i32; 3],
    registry: &MigrationRegistry,
) -> io::Result<Option<ChunkPayload>> {
    read_chunk_from(&chunk_path_in_dir(base, slug, dim, coord), registry)
}

fn write_chunk_to(path: &Path, payload: &ChunkPayload) -> io::Result<()> {
    let cfg = bincode::config::standard();
    let raw = bincode::serde::encode_to_vec(payload, cfg)
        .map_err(|err| invalid(format!("chunk payload encode failed: {err}")))?;
    let uncompressed_len = raw.len() as u64;

    // Try zstd. If the compressor actually shrinks the payload,
    // ship the compressed bytes; otherwise fall back to raw to avoid
    // paying the decompression cost on a no-win chunk.
    let (kind, body): (u8, Vec<u8>) = match zstd::encode_all(&raw[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw),
    };

    let mut bytes: Vec<u8> = Vec::with_capacity(body.len() + 17);
    bytes.extend_from_slice(CHUNK_MAGIC);
    bytes.extend_from_slice(&CHUNK_FORMAT_VERSION.to_le_bytes());
    bytes.push(kind);
    bytes.extend_from_slice(&uncompressed_len.to_le_bytes());
    bytes.extend_from_slice(&body);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    atomic_save(path, &bytes)
}

fn read_chunk_from(path: &Path, registry: &MigrationRegistry) -> io::Result<Option<ChunkPayload>> {
    let migrated_at_path = std::cell::Cell::new(false);
    let migrated_bytes_holder: std::cell::RefCell<Option<Vec<u8>>> = std::cell::RefCell::new(None);
    let parse = |bytes: &[u8]| -> io::Result<ChunkPayload> {
        let (payload, did_migrate, new_bytes) = decode_chunk_bytes(bytes, path, registry)?;
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
/// Returns `(payload, did_migrate, post_migration_bytes)`. The
/// post-migration bytes equal the input if no migration ran; on a
/// migrated read the caller writes them back via `atomic_save` so the
/// next load skips the chain.
fn decode_chunk_bytes(
    bytes: &[u8],
    path: &Path,
    registry: &MigrationRegistry,
) -> io::Result<(ChunkPayload, bool, Vec<u8>)> {
    if bytes.len() < 17 {
        return Err(invalid("chunk file truncated header"));
    }
    if &bytes[..4] != CHUNK_MAGIC {
        return Err(invalid("chunk file magic mismatch"));
    }
    let disk_version = migrate::peek_envelope_version(bytes)?;
    let did_migrate = disk_version != CHUNK_FORMAT_VERSION;
    let migrated = migrate::migrate_to(
        registry,
        SaveKind::Chunk,
        CHUNK_FORMAT_VERSION,
        path,
        disk_version,
        bytes,
    )?;
    let post = migrated.as_slice();
    if post.len() < 17 {
        return Err(invalid("chunk file truncated header (post-migration)"));
    }
    if &post[..4] != CHUNK_MAGIC {
        return Err(invalid("chunk file magic mismatch (post-migration)"));
    }
    let version = u32::from_le_bytes([post[4], post[5], post[6], post[7]]);
    if version != CHUNK_FORMAT_VERSION {
        return Err(invalid(format!(
            "chunk format_version {version} not supported (expected {CHUNK_FORMAT_VERSION})"
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
                return Err(invalid("uncompressed chunk body length mismatch"));
            }
            body.to_vec()
        }
        COMP_ZSTD => decode_zstd(body, uncompressed_len)?,
        other => {
            return Err(invalid(format!("unknown chunk compression kind {other}")));
        }
    };

    let cfg = bincode::config::standard();
    let (payload, _read) = bincode::serde::decode_from_slice::<ChunkPayload, _>(&raw, cfg)
        .map_err(|err| invalid(format!("chunk payload decode failed: {err}")))?;
    Ok((payload, did_migrate, migrated))
}

/// Decompress `body` via zstd into a buffer of exactly
/// `expected_len` bytes. Refuses to grow the buffer past
/// `expected_len` so a maliciously inflated header can't cause an
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

/// Build a fresh `ChunkPayload` representing a uniformly-air chunk
/// at the given coord. Useful for tests + as a baseline for
/// hand-constructing payloads outside the normal worldgen path.
pub(crate) fn empty_payload(coord: [i32; 3]) -> ChunkPayload {
    ChunkPayload {
        format_version: CHUNK_PAYLOAD_VERSION,
        coord,
        voxels: ChunkVoxels::from_chunk(&Chunk::empty()),
        liquid_levels: Vec::new(),
        torch_orient: Vec::new(),
        snow_layers: Vec::new(),
    }
}

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

/// Inverse of `Chunk::index`. Given a flat index into
/// `[BlockId; CHUNK_VOXELS]`, return the `(x, y, z)` triple that
/// `Chunk::index` would have flattened.
fn unflatten(i: usize) -> (usize, usize, usize) {
    let x = i % CHUNK_SIZE;
    let y = (i / CHUNK_SIZE) % CHUNK_SIZE;
    let z = i / (CHUNK_SIZE * CHUNK_SIZE);
    (x, y, z)
}
