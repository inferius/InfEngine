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
/// changes in a breaking way.
///
/// - v1: chest-slots only.
/// - v2: + `furnaces: Vec<FurnaceSave>` (slot vec + runtime triple).
///   Migration v1 -> v2 lives in `containers_v1_to_v2` and registers
///   into `default_registry()` at first read.
/// - v3: + `fires: Vec<FireSave>` (per-cell age for Q.4b/c/d/e fires).
///   Migration v2 -> v3 lives in `containers_v2_to_v3` and registers
///   into `default_registry()` at first read.
/// - v4 (W138): + `doors` (facing byte incl. the trapdoor half bit +
///   open flag), `chest_facings`, `brewing` (slots + progress) and
///   `levers_on`. None of these runtime maps were persisted before -
///   doors reloaded with facing 0 + closed (the user's "visually
///   open reversed AND not passable after load" report), chests lost
///   their latch direction, brewing stands lost their contents and
///   levers reset to off. Migration v3 -> v4 lives in
///   `containers_v3_to_v4`.
/// - v5 (W143): + `rails` (per-rail stored `RailShape` byte). Rail
///   shapes became real block state with the stateful MC RailState
///   port; without persistence every junction re-derived (and
///   re-bent) on load. Migration v4 -> v5 lives in
///   `containers_v4_to_v5`; legacy rails backfill from the old
///   derivation on first remesh.
pub(crate) const CONTAINERS_FORMAT_VERSION: u32 = 5;

/// Default zstd compression level. Matches the sibling save modules.
const ZSTD_LEVEL: i32 = 3;

/// Compression-kind discriminant byte stored in the envelope.
const COMP_NONE: u8 = 0;
const COMP_ZSTD: u8 = 1;

/// All persistent block-state for one dimension.
///
/// One file per dimension (e.g. `containers/overworld.bin`). Today
/// the per-position state covers chests + furnaces; future per-pos
/// state (signs, jukeboxes, ...) lands as new fields alongside
/// `chests` + `furnaces`. Adding a non-default-compatible field
/// bumps `format_version` and registers a migration step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct DimensionContainers {
    /// Payload-level format version. Matches the envelope version
    /// (both bump together).
    pub format_version: u32,
    /// Every chest with non-default state in the dimension. An
    /// empty chest still gets persisted so a freshly-placed empty
    /// chest survives a round trip (the world block alone doesn't
    /// know the slot count it should mint).
    pub chests: Vec<ChestSave>,
    /// Every furnace with non-default state in the dimension. Each
    /// entry carries the 3 slots (input / fuel / output) + the live
    /// runtime triple (fuel_left_ms, fuel_full_ms, cook_progress_ms)
    /// so a mid-cook save resumes cleanly.
    pub furnaces: Vec<FurnaceSave>,
    /// Every fire cell in the dimension. Carries the per-cell age
    /// so a reload resumes the spread algorithm at the same point
    /// (no offline catch-up - fires don't tick while the world is
    /// unloaded, matching MC).
    pub fires: Vec<FireSave>,
    /// W138 (v4): every door + trapdoor with recorded state. The
    /// world blocks reload from chunk data; this carries the
    /// parallel facing/open maps the renderer + collision read.
    pub doors: Vec<DoorSave>,
    /// W138 (v4): per-chest latch facing (W126 render state).
    pub chest_facings: Vec<FacingSave>,
    /// W138 (v4): brewing stands (slots + progress) - they were
    /// silently dropped on save before.
    pub brewing: Vec<BrewingSave>,
    /// W138 (v4): every lever currently switched ON.
    pub levers_on: Vec<[i32; 3]>,
    /// W143 (v5): per-rail stored shape (MC RailShape block state).
    pub rails: Vec<RailShapeSave>,
}

impl DimensionContainers {
    /// Build a fresh empty bundle at the current format version.
    pub(crate) fn empty() -> Self {
        Self {
            format_version: CONTAINERS_FORMAT_VERSION,
            chests: Vec::new(),
            furnaces: Vec::new(),
            fires: Vec::new(),
            doors: Vec::new(),
            chest_facings: Vec::new(),
            brewing: Vec::new(),
            levers_on: Vec::new(),
            rails: Vec::new(),
        }
    }
}

/// W143 (v5): one stored rail shape (see `block::rail` shape codes).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct RailShapeSave {
    /// Absolute world voxel coords of the rail block.
    pub pos: [i32; 3],
    /// RailShape code 0..=9 (bit 7 power flag is derived, not stored).
    pub shape: u8,
}

/// W138 (v4): one persistent door / trapdoor state entry. `facing`
/// is the runtime byte verbatim: bits 0-1 = facing code, bit 2 =
/// trapdoor top-half (doors ignore it). `open` mirrors membership
/// in `Voxel::door_open` (keyed by the LOWER half for doors).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct DoorSave {
    /// Absolute world voxel coords (door lower half / trapdoor cell).
    pub pos: [i32; 3],
    /// Runtime facing byte (see `block::door`).
    pub facing: u8,
    /// True when the door/trapdoor is open.
    pub open: bool,
}

/// W138 (v4): a bare per-position facing byte (chest latch today).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct FacingSave {
    /// Absolute world voxel coords.
    pub pos: [i32; 3],
    /// Facing code (0=+X, 1=+Z, 2=-X, 3=-Z).
    pub facing: u8,
}

/// W138 (v4): one persistent brewing stand (5 slots + progress).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct BrewingSave {
    /// Absolute world voxel coords of the stand block.
    pub pos: [i32; 3],
    /// Slot contents in slot-index order (ingredient, fuel, 3
    /// bottles - the length is recorded so the layout can grow).
    pub slots: Vec<Option<(u16, u8)>>,
    /// Accumulated brew progress in milliseconds.
    pub progress_ms: u32,
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

/// One persistent fire entry. Q.4f.
///
/// `age` mirrors `Voxel::fires[pos]` (0..=15, the MC FireBlock
/// AGE property). The world block at `pos` is implicitly
/// `FIRE_ID` - we don't persist the block id because the loader
/// re-derives it from chunk data; this side bundle only carries
/// the per-cell tick state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct FireSave {
    /// Absolute world voxel coords of the fire cell.
    pub pos: [i32; 3],
    /// 0..=15. Saturates at 15 (MC's MAX_AGE).
    pub age: u8,
}

/// One persistent furnace entry.
///
/// Mirrors `ChestSave` shape but with a fixed 3-slot layout
/// (input / fuel / output) and the live runtime triple. The slot
/// vec keeps its `Vec<Option<...>>` shape so a future "blast
/// furnace" variant can ship with more slots without touching the
/// schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct FurnaceSave {
    /// Absolute world voxel coords of the furnace block.
    pub pos: [i32; 3],
    /// Slot contents in slot-index order: 0 = input, 1 = fuel,
    /// 2 = output.
    pub slots: Vec<Option<(u16, u8)>>,
    /// Remaining burn time on the current fuel item, in
    /// milliseconds.
    pub fuel_left_ms: u32,
    /// Total burn time of the current fuel item, in milliseconds
    /// (used by the UI fuel bar).
    pub fuel_full_ms: u32,
    /// Accumulated cook progress in milliseconds. Reaches
    /// `FURNACE_COOK_MS_PER_OUTPUT` (10 000) to emit one output.
    pub cook_progress_ms: u32,
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

// ---------------------------------------------------------------
// Migration: v1 -> v2 (adds the `furnaces` field).
// ---------------------------------------------------------------

/// V1 on-disk shape (chest-slots only). Decoded just long enough
/// to convert into the v2 shape during migration; never used at
/// runtime past the migration step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct DimensionContainersV1 {
    pub format_version: u32,
    pub chests: Vec<ChestSave>,
}

/// V2 on-disk shape (chests + furnaces). Decoded just long enough
/// to convert into the v3 shape during migration; never used at
/// runtime past the migration step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct DimensionContainersV2 {
    pub format_version: u32,
    pub chests: Vec<ChestSave>,
    pub furnaces: Vec<FurnaceSave>,
}

/// V3 on-disk shape (+ fires). Decoded just long enough to convert
/// into the v4 shape during migration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct DimensionContainersV3 {
    pub format_version: u32,
    pub chests: Vec<ChestSave>,
    pub furnaces: Vec<FurnaceSave>,
    pub fires: Vec<FireSave>,
}

/// V4 on-disk shape (+ doors / chest facings / brewing / levers).
/// FROZEN: the live struct moved on to v5 (rails); the v3->v4 and
/// v4->v5 arrows must encode/decode exactly this shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct DimensionContainersV4 {
    pub format_version: u32,
    pub chests: Vec<ChestSave>,
    pub furnaces: Vec<FurnaceSave>,
    pub fires: Vec<FireSave>,
    pub doors: Vec<DoorSave>,
    pub chest_facings: Vec<FacingSave>,
    pub brewing: Vec<BrewingSave>,
    pub levers_on: Vec<[i32; 3]>,
}

/// One-step v1 -> v2 migration arrow. Takes the raw envelope
/// (magic + version + comp_kind + uncompressed_len + body) of a
/// v1 file, returns the raw envelope of an equivalent v2 file.
///
/// Wire steps:
/// 1. Decode the envelope header. Refuse anything that's not a
///    v1 containers envelope.
/// 2. Decompress the body (zstd or none).
/// 3. Bincode-decode the v1 payload (chests only).
/// 4. Re-encode as a v2 payload with `furnaces: vec![]`.
/// 5. Re-compress + re-build the envelope at v2.
///
/// Mirrors how the existing `*_with_registry` test arrows work,
/// but production-shaped: real serde decode of the old payload +
/// re-encode of the new shape, not just a header byte flip.
pub(crate) fn containers_v1_to_v2(bytes: &[u8]) -> io::Result<Vec<u8>> {
    // ---- 1. Envelope header. ----
    if bytes.len() < 17 {
        return Err(invalid("containers v1 envelope truncated"));
    }
    if &bytes[..4] != CONTAINERS_MAGIC {
        return Err(invalid("containers v1 envelope magic mismatch"));
    }
    let on_disk_v = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if on_disk_v != 1 {
        return Err(invalid(format!(
            "containers_v1_to_v2 expected v1, got v{on_disk_v}"
        )));
    }
    let kind = bytes[8];
    let uncompressed_len = u64::from_le_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        bytes[16],
    ]) as usize;
    let body = &bytes[17..];

    // ---- 2. Body decompression. ----
    let raw_v1: Vec<u8> = match kind {
        COMP_NONE => {
            if body.len() != uncompressed_len {
                return Err(invalid("v1 body length mismatch during migration"));
            }
            body.to_vec()
        }
        COMP_ZSTD => decode_zstd(body, uncompressed_len)?,
        other => {
            return Err(invalid(format!(
                "unknown v1 compression kind {other} during migration"
            )));
        }
    };

    // ---- 3. Decode v1 payload. ----
    let cfg = bincode::config::standard();
    let (v1, _read) = bincode::serde::decode_from_slice::<DimensionContainersV1, _>(&raw_v1, cfg)
        .map_err(|err| invalid(format!("v1 payload decode failed: {err}")))?;

    // ---- 4. Re-encode as v2 (using the V2 shape so a future
    //         v3 field doesn't poison this arrow's payload). ----
    let v2 = DimensionContainersV2 {
        format_version: 2,
        chests: v1.chests,
        furnaces: Vec::new(),
    };
    let raw_v2 = bincode::serde::encode_to_vec(&v2, cfg)
        .map_err(|err| invalid(format!("v2 payload encode failed: {err}")))?;
    let uncompressed_len_v2 = raw_v2.len() as u64;

    // ---- 5. Re-compress + re-envelope. ----
    let (kind_v2, body_v2): (u8, Vec<u8>) = match zstd::encode_all(&raw_v2[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw_v2.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw_v2),
    };
    let mut out: Vec<u8> = Vec::with_capacity(body_v2.len() + 17);
    out.extend_from_slice(CONTAINERS_MAGIC);
    out.extend_from_slice(&2u32.to_le_bytes());
    out.push(kind_v2);
    out.extend_from_slice(&uncompressed_len_v2.to_le_bytes());
    out.extend_from_slice(&body_v2);
    Ok(out)
}

// ---------------------------------------------------------------
// Migration: v2 -> v3 (adds the `fires` field for Q.4f).
// ---------------------------------------------------------------

/// One-step v2 -> v3 migration arrow. Same shape as v1->v2: decode
/// the v2 envelope, re-encode as v3 with `fires: vec![]`.
pub(crate) fn containers_v2_to_v3(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() < 17 {
        return Err(invalid("containers v2 envelope truncated"));
    }
    if &bytes[..4] != CONTAINERS_MAGIC {
        return Err(invalid("containers v2 envelope magic mismatch"));
    }
    let on_disk_v = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if on_disk_v != 2 {
        return Err(invalid(format!(
            "containers_v2_to_v3 expected v2, got v{on_disk_v}"
        )));
    }
    let kind = bytes[8];
    let uncompressed_len = u64::from_le_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        bytes[16],
    ]) as usize;
    let body = &bytes[17..];
    let raw_v2: Vec<u8> = match kind {
        COMP_NONE => {
            if body.len() != uncompressed_len {
                return Err(invalid("v2 body length mismatch during migration"));
            }
            body.to_vec()
        }
        COMP_ZSTD => decode_zstd(body, uncompressed_len)?,
        other => {
            return Err(invalid(format!(
                "unknown v2 compression kind {other} during migration"
            )));
        }
    };
    let cfg = bincode::config::standard();
    let (v2, _read) = bincode::serde::decode_from_slice::<DimensionContainersV2, _>(&raw_v2, cfg)
        .map_err(|err| invalid(format!("v2 payload decode failed: {err}")))?;
    // W138: encode the FROZEN v3 shape, not the live struct - the
    // live struct moved on to v4 (doors/facings/brewing/levers) and
    // would poison this arrow's payload.
    let v3 = DimensionContainersV3 {
        format_version: 3,
        chests: v2.chests,
        furnaces: v2.furnaces,
        fires: Vec::new(),
    };
    let raw_v3 = bincode::serde::encode_to_vec(&v3, cfg)
        .map_err(|err| invalid(format!("v3 payload encode failed: {err}")))?;
    let uncompressed_len_v3 = raw_v3.len() as u64;
    let (kind_v3, body_v3): (u8, Vec<u8>) = match zstd::encode_all(&raw_v3[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw_v3.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw_v3),
    };
    let mut out: Vec<u8> = Vec::with_capacity(body_v3.len() + 17);
    out.extend_from_slice(CONTAINERS_MAGIC);
    out.extend_from_slice(&3u32.to_le_bytes());
    out.push(kind_v3);
    out.extend_from_slice(&uncompressed_len_v3.to_le_bytes());
    out.extend_from_slice(&body_v3);
    Ok(out)
}

// ---------------------------------------------------------------
// Migration: v3 -> v4 (W138 - doors / chest facings / brewing /
// levers).
// ---------------------------------------------------------------

/// One-step v3 -> v4 migration arrow. Same shape as the earlier
/// arrows: decode the v3 envelope, re-encode as v4 with the four
/// new collections empty (old saves simply had no recorded state -
/// doors degrade to the same default they showed before W138).
pub(crate) fn containers_v3_to_v4(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() < 17 {
        return Err(invalid("containers v3 envelope truncated"));
    }
    if &bytes[..4] != CONTAINERS_MAGIC {
        return Err(invalid("containers v3 envelope magic mismatch"));
    }
    let on_disk_v = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if on_disk_v != 3 {
        return Err(invalid(format!(
            "containers_v3_to_v4 expected v3, got v{on_disk_v}"
        )));
    }
    let kind = bytes[8];
    let uncompressed_len = u64::from_le_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        bytes[16],
    ]) as usize;
    let body = &bytes[17..];
    let raw_v3: Vec<u8> = match kind {
        COMP_NONE => {
            if body.len() != uncompressed_len {
                return Err(invalid("v3 body length mismatch during migration"));
            }
            body.to_vec()
        }
        COMP_ZSTD => decode_zstd(body, uncompressed_len)?,
        other => {
            return Err(invalid(format!(
                "unknown v3 compression kind {other} during migration"
            )));
        }
    };
    let cfg = bincode::config::standard();
    let (v3, _read) = bincode::serde::decode_from_slice::<DimensionContainersV3, _>(&raw_v3, cfg)
        .map_err(|err| invalid(format!("v3 payload decode failed: {err}")))?;
    // W143: encode the FROZEN v4 shape, not the live struct - the
    // live struct moved on to v5 (rails) and would poison this
    // arrow's payload.
    let v4 = DimensionContainersV4 {
        format_version: 4,
        chests: v3.chests,
        furnaces: v3.furnaces,
        fires: v3.fires,
        doors: Vec::new(),
        chest_facings: Vec::new(),
        brewing: Vec::new(),
        levers_on: Vec::new(),
    };
    let raw_v4 = bincode::serde::encode_to_vec(&v4, cfg)
        .map_err(|err| invalid(format!("v4 payload encode failed: {err}")))?;
    let uncompressed_len_v4 = raw_v4.len() as u64;
    let (kind_v4, body_v4): (u8, Vec<u8>) = match zstd::encode_all(&raw_v4[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw_v4.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw_v4),
    };
    let mut out: Vec<u8> = Vec::with_capacity(body_v4.len() + 17);
    out.extend_from_slice(CONTAINERS_MAGIC);
    out.extend_from_slice(&4u32.to_le_bytes());
    out.push(kind_v4);
    out.extend_from_slice(&uncompressed_len_v4.to_le_bytes());
    out.extend_from_slice(&body_v4);
    Ok(out)
}

// ---------------------------------------------------------------
// Migration: v4 -> v5 (W143 - stored rail shapes).
// ---------------------------------------------------------------

/// One-step v4 -> v5 migration arrow: decode the frozen v4 payload,
/// re-encode as v5 with `rails: vec![]` (legacy rails backfill from
/// the stateless derivation on first remesh, freezing their current
/// look).
pub(crate) fn containers_v4_to_v5(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() < 17 {
        return Err(invalid("containers v4 envelope truncated"));
    }
    if &bytes[..4] != CONTAINERS_MAGIC {
        return Err(invalid("containers v4 envelope magic mismatch"));
    }
    let on_disk_v = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if on_disk_v != 4 {
        return Err(invalid(format!(
            "containers_v4_to_v5 expected v4, got v{on_disk_v}"
        )));
    }
    let kind = bytes[8];
    let uncompressed_len = u64::from_le_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        bytes[16],
    ]) as usize;
    let body = &bytes[17..];
    let raw_v4: Vec<u8> = match kind {
        COMP_NONE => {
            if body.len() != uncompressed_len {
                return Err(invalid("v4 body length mismatch during migration"));
            }
            body.to_vec()
        }
        COMP_ZSTD => decode_zstd(body, uncompressed_len)?,
        other => {
            return Err(invalid(format!(
                "unknown v4 compression kind {other} during migration"
            )));
        }
    };
    let cfg = bincode::config::standard();
    let (v4, _read) = bincode::serde::decode_from_slice::<DimensionContainersV4, _>(&raw_v4, cfg)
        .map_err(|err| invalid(format!("v4 payload decode failed: {err}")))?;
    let v5 = DimensionContainers {
        format_version: 5,
        chests: v4.chests,
        furnaces: v4.furnaces,
        fires: v4.fires,
        doors: v4.doors,
        chest_facings: v4.chest_facings,
        brewing: v4.brewing,
        levers_on: v4.levers_on,
        rails: Vec::new(),
    };
    let raw_v5 = bincode::serde::encode_to_vec(&v5, cfg)
        .map_err(|err| invalid(format!("v5 payload encode failed: {err}")))?;
    let uncompressed_len_v5 = raw_v5.len() as u64;
    let (kind_v5, body_v5): (u8, Vec<u8>) = match zstd::encode_all(&raw_v5[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw_v5.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw_v5),
    };
    let mut out: Vec<u8> = Vec::with_capacity(body_v5.len() + 17);
    out.extend_from_slice(CONTAINERS_MAGIC);
    out.extend_from_slice(&5u32.to_le_bytes());
    out.push(kind_v5);
    out.extend_from_slice(&uncompressed_len_v5.to_le_bytes());
    out.extend_from_slice(&body_v5);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_constructor_uses_current_format_version() {
        let c = DimensionContainers::empty();
        assert_eq!(c.format_version, CONTAINERS_FORMAT_VERSION);
        assert!(c.chests.is_empty());
        assert!(c.doors.is_empty());
        assert!(c.brewing.is_empty());
    }

    /// W138: a hand-built v3 envelope must migrate to v4 with the
    /// new collections empty and the old ones intact.
    #[test]
    fn v3_envelope_migrates_to_v4() {
        let v3 = DimensionContainersV3 {
            format_version: 3,
            chests: vec![ChestSave {
                pos: [1, 2, 3],
                slots: vec![None, Some((4, 5))],
            }],
            furnaces: Vec::new(),
            fires: vec![FireSave { pos: [7, 8, 9], age: 3 }],
        };
        let cfg = bincode::config::standard();
        let raw = bincode::serde::encode_to_vec(&v3, cfg).unwrap();
        let mut file: Vec<u8> = Vec::new();
        file.extend_from_slice(CONTAINERS_MAGIC);
        file.extend_from_slice(&3u32.to_le_bytes());
        file.push(COMP_NONE);
        file.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        file.extend_from_slice(&raw);

        let v4_bytes = containers_v3_to_v4(&file).expect("migration");
        let version =
            u32::from_le_bytes([v4_bytes[4], v4_bytes[5], v4_bytes[6], v4_bytes[7]]);
        assert_eq!(version, 4);
        let kind = v4_bytes[8];
        let len = u64::from_le_bytes([
            v4_bytes[9], v4_bytes[10], v4_bytes[11], v4_bytes[12], v4_bytes[13],
            v4_bytes[14], v4_bytes[15], v4_bytes[16],
        ]) as usize;
        let body = &v4_bytes[17..];
        let raw2 = if kind == COMP_ZSTD {
            decode_zstd(body, len).unwrap()
        } else {
            body.to_vec()
        };
        // W143: the arrow encodes the FROZEN v4 shape (live moved
        // to v5), so decode it as that.
        let (v4, _) = bincode::serde::decode_from_slice::<DimensionContainersV4, _>(
            &raw2, cfg,
        )
        .unwrap();
        assert_eq!(v4.format_version, 4);
        assert_eq!(v4.chests.len(), 1);
        assert_eq!(v4.fires.len(), 1);
        assert!(v4.doors.is_empty());
        assert!(v4.levers_on.is_empty());
    }

    /// W143: a hand-built v4 envelope must migrate to v5 with the
    /// rails empty and every v4 collection intact.
    #[test]
    fn v4_envelope_migrates_to_v5() {
        let v4 = DimensionContainersV4 {
            format_version: 4,
            chests: vec![ChestSave {
                pos: [1, 2, 3],
                slots: vec![None, Some((4, 5))],
            }],
            furnaces: Vec::new(),
            fires: Vec::new(),
            doors: vec![DoorSave { pos: [4, 5, 6], facing: 2, open: true }],
            chest_facings: Vec::new(),
            brewing: Vec::new(),
            levers_on: vec![[9, 9, 9]],
        };
        let cfg = bincode::config::standard();
        let raw = bincode::serde::encode_to_vec(&v4, cfg).unwrap();
        let mut file: Vec<u8> = Vec::new();
        file.extend_from_slice(CONTAINERS_MAGIC);
        file.extend_from_slice(&4u32.to_le_bytes());
        file.push(COMP_NONE);
        file.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        file.extend_from_slice(&raw);

        let v5_bytes = containers_v4_to_v5(&file).expect("migration");
        let version =
            u32::from_le_bytes([v5_bytes[4], v5_bytes[5], v5_bytes[6], v5_bytes[7]]);
        assert_eq!(version, 5);
        let kind = v5_bytes[8];
        let len = u64::from_le_bytes([
            v5_bytes[9], v5_bytes[10], v5_bytes[11], v5_bytes[12], v5_bytes[13],
            v5_bytes[14], v5_bytes[15], v5_bytes[16],
        ]) as usize;
        let body = &v5_bytes[17..];
        let raw2 = if kind == COMP_ZSTD {
            decode_zstd(body, len).unwrap()
        } else {
            body.to_vec()
        };
        let (v5, _) = bincode::serde::decode_from_slice::<DimensionContainers, _>(
            &raw2, cfg,
        )
        .unwrap();
        assert_eq!(v5.format_version, 5);
        assert_eq!(v5.chests.len(), 1);
        assert_eq!(v5.doors.len(), 1);
        assert_eq!(v5.levers_on.len(), 1);
        assert!(v5.rails.is_empty());
    }
}
