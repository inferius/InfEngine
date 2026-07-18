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
/// - v1: mobs + drops.
/// - v2 (W137): adds `carts` + `next_cart_id` (minecarts persist).
///   Migration lives in `entities_v1_to_v2`, registered in
///   `migrate::register_production_arrows`.
/// - v3 (W302): adds `structure_mob_anchors` (template-entity spawn
///   dedup - villages stay populated exactly once) + per-mob
///   `persistent` / `baby_left` (villagers + pen animals survive
///   reload as non-despawning; babies keep growing).
pub(crate) const ENTITIES_FORMAT_VERSION: u32 = 3;

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
    /// W137 (v2): every minecart riding the dim's rails.
    pub carts: Vec<CartSave>,
    /// W137 (v2): monotonic id source for the next minted cart.
    pub next_cart_id: u32,
    /// W302 (v3): template-entity anchors already spawned. A chunk
    /// that regenerates (streamed out + revisited) checks this set
    /// before minting its embedded villagers / pen animals.
    pub structure_mob_anchors: Vec<[i32; 3]>,
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
            carts: Vec::new(),
            next_cart_id: 0,
            structure_mob_anchors: Vec::new(),
        }
    }
}

/// W137 (v2): one persistent minecart entry. The cart is bound to a
/// rail CELL + a parameter along that cell's path (see
/// `minecart::Minecart`); world position is re-derived on load via
/// `snap_to_rail`, so only the path binding + motion persist.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct CartSave {
    /// Stable per-save cart id. Allocated from
    /// `DimensionEntities::next_cart_id`.
    pub id: u32,
    /// Rail cell the cart is bound to (world voxel coords).
    pub cell: [i32; 3],
    /// Param along the cell's path, 0..1.
    pub t: f32,
    /// Signed speed (m/s) along +t.
    pub vel: f32,
    /// Heading - rotation around Y, radians (kept so a parked cart
    /// faces the same way after reload).
    pub yaw: f32,
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
    /// W302 (v3): MC `PersistenceRequired` - structure-spawned mobs
    /// (villagers, pen animals) never despawn.
    pub persistent: bool,
    /// W302 (v3): seconds of babyhood left (0 = adult). Was runtime
    /// only before v3 - babies reloaded as adults.
    pub baby_left: f32,
    // Future fields go here - with a version bump + migration arrow
    // (bincode is positional; `#[serde(default)]` does NOT cover it).
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

// ---------------------------------------------------------------
// Migration: v1 -> v2 (W137 - adds `carts` + `next_cart_id`).
// ---------------------------------------------------------------

/// Frozen snapshot of the v1 payload shape (mobs + drops only).
/// Kept private to the migration arrow so the live struct can keep
/// evolving without poisoning old decodes.
#[derive(Debug, Deserialize)]
struct DimensionEntitiesV1 {
    #[allow(dead_code)]
    format_version: u32,
    mobs: Vec<MobSaveV2>,
    drops: Vec<DropSave>,
    next_mob_id: u32,
    next_drop_id: u32,
}

/// W302: the v2 on-disk mob shape (pre-`persistent`/`baby_left`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MobSaveV2 {
    id: u32,
    kind: u8,
    pos: [f32; 3],
    yaw: f32,
    vel: [f32; 3],
    health: f32,
    age_seconds: f32,
}

/// W302: the v2 on-disk dimension shape (pre-anchor-set).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct DimensionEntitiesV2 {
    format_version: u32,
    mobs: Vec<MobSaveV2>,
    drops: Vec<DropSave>,
    next_mob_id: u32,
    next_drop_id: u32,
    carts: Vec<CartSave>,
    next_cart_id: u32,
}

impl MobSaveV2 {
    /// Lift to the current shape with the v3 defaults.
    fn lift(self) -> MobSave {
        MobSave {
            id: self.id,
            kind: self.kind,
            pos: self.pos,
            yaw: self.yaw,
            vel: self.vel,
            health: self.health,
            age_seconds: self.age_seconds,
            persistent: false,
            baby_left: 0.0,
        }
    }
}

/// One-step v1 -> v2 migration arrow: decode the v1 envelope,
/// re-encode as v2 with `carts: vec![]` / `next_cart_id: 0`. Same
/// shape as `containers::containers_v1_to_v2` (the production
/// precedent).
pub(crate) fn entities_v1_to_v2(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() < 17 {
        return Err(invalid("entities v1 envelope truncated"));
    }
    if &bytes[..4] != ENTITIES_MAGIC {
        return Err(invalid("entities v1 envelope magic mismatch"));
    }
    let on_disk_v = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if on_disk_v != 1 {
        return Err(invalid(format!(
            "entities_v1_to_v2 expected v1, got v{on_disk_v}"
        )));
    }
    let kind = bytes[8];
    let uncompressed_len = u64::from_le_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15], bytes[16],
    ]) as usize;
    let body = &bytes[17..];
    let raw_v1: Vec<u8> = match kind {
        COMP_NONE => {
            if body.len() != uncompressed_len {
                return Err(invalid("entities v1 body length mismatch during migration"));
            }
            body.to_vec()
        }
        COMP_ZSTD => decode_zstd(body, uncompressed_len)?,
        other => {
            return Err(invalid(format!(
                "unknown entities v1 compression kind {other} during migration"
            )));
        }
    };
    let cfg = bincode::config::standard();
    let (v1, _read) = bincode::serde::decode_from_slice::<DimensionEntitiesV1, _>(&raw_v1, cfg)
        .map_err(|err| invalid(format!("entities v1 payload decode failed: {err}")))?;
    // W302: emit the TRUE v2 shape - encoding the current (v3)
    // struct under a v2 tag would break the chained 2 -> 3 arrow.
    let v2 = DimensionEntitiesV2 {
        format_version: 2,
        mobs: v1.mobs,
        drops: v1.drops,
        next_mob_id: v1.next_mob_id,
        next_drop_id: v1.next_drop_id,
        carts: Vec::new(),
        next_cart_id: 0,
    };
    let raw_v2 = bincode::serde::encode_to_vec(&v2, cfg)
        .map_err(|err| invalid(format!("entities v2 payload encode failed: {err}")))?;
    let uncompressed_len_v2 = raw_v2.len() as u64;
    let (kind_v2, body_v2): (u8, Vec<u8>) = match zstd::encode_all(&raw_v2[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw_v2.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw_v2),
    };
    let mut out: Vec<u8> = Vec::with_capacity(body_v2.len() + 17);
    out.extend_from_slice(ENTITIES_MAGIC);
    out.extend_from_slice(&2u32.to_le_bytes());
    out.push(kind_v2);
    out.extend_from_slice(&uncompressed_len_v2.to_le_bytes());
    out.extend_from_slice(&body_v2);
    Ok(out)
}

/// W302: one-step v2 -> v3 migration arrow. Decodes the v2 envelope,
/// lifts every mob with `persistent: false` / `baby_left: 0` and adds
/// an empty `structure_mob_anchors` set. Same envelope walk as
/// `entities_v1_to_v2`.
pub(crate) fn entities_v2_to_v3(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() < 17 {
        return Err(invalid("entities v2 envelope truncated"));
    }
    if &bytes[..4] != ENTITIES_MAGIC {
        return Err(invalid("entities v2 envelope magic mismatch"));
    }
    let on_disk_v = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if on_disk_v != 2 {
        return Err(invalid(format!(
            "entities_v2_to_v3 expected v2, got v{on_disk_v}"
        )));
    }
    let kind = bytes[8];
    let uncompressed_len = u64::from_le_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15], bytes[16],
    ]) as usize;
    let body = &bytes[17..];
    let raw_v2: Vec<u8> = match kind {
        COMP_NONE => {
            if body.len() != uncompressed_len {
                return Err(invalid("entities v2 body length mismatch during migration"));
            }
            body.to_vec()
        }
        COMP_ZSTD => decode_zstd(body, uncompressed_len)?,
        other => {
            return Err(invalid(format!(
                "unknown entities v2 compression kind {other} during migration"
            )));
        }
    };
    let cfg = bincode::config::standard();
    let (v2, _read) = bincode::serde::decode_from_slice::<DimensionEntitiesV2, _>(&raw_v2, cfg)
        .map_err(|err| invalid(format!("entities v2 payload decode failed: {err}")))?;
    let v3 = DimensionEntities {
        format_version: 3,
        mobs: v2.mobs.into_iter().map(MobSaveV2::lift).collect(),
        drops: v2.drops,
        next_mob_id: v2.next_mob_id,
        next_drop_id: v2.next_drop_id,
        carts: v2.carts,
        next_cart_id: v2.next_cart_id,
        structure_mob_anchors: Vec::new(),
    };
    let raw_v3 = bincode::serde::encode_to_vec(&v3, cfg)
        .map_err(|err| invalid(format!("entities v3 payload encode failed: {err}")))?;
    let uncompressed_len_v3 = raw_v3.len() as u64;
    let (kind_v3, body_v3): (u8, Vec<u8>) = match zstd::encode_all(&raw_v3[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw_v3.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw_v3),
    };
    let mut out: Vec<u8> = Vec::with_capacity(body_v3.len() + 17);
    out.extend_from_slice(ENTITIES_MAGIC);
    out.extend_from_slice(&3u32.to_le_bytes());
    out.push(kind_v3);
    out.extend_from_slice(&uncompressed_len_v3.to_le_bytes());
    out.extend_from_slice(&body_v3);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// W302: the v2 -> v3 arrow lifts mobs with persistence defaults
    /// and adds the empty anchor set.
    #[test]
    fn entities_v2_to_v3_lifts_mobs_and_adds_anchor_set() {
        let cfg = bincode::config::standard();
        let v2 = DimensionEntitiesV2 {
            format_version: 2,
            mobs: vec![MobSaveV2 {
                id: 7,
                kind: 11,
                pos: [1.0, 2.0, 3.0],
                yaw: 0.5,
                vel: [0.0; 3],
                health: 20.0,
                age_seconds: 0.0,
            }],
            drops: Vec::new(),
            next_mob_id: 8,
            next_drop_id: 1,
            carts: Vec::new(),
            next_cart_id: 1,
        };
        let raw = bincode::serde::encode_to_vec(&v2, cfg).unwrap();
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(ENTITIES_MAGIC);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.push(COMP_NONE);
        bytes.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&raw);
        let out = entities_v2_to_v3(&bytes).expect("arrow lifts");
        assert_eq!(&out[..4], ENTITIES_MAGIC);
        assert_eq!(u32::from_le_bytes([out[4], out[5], out[6], out[7]]), 3);
        let kind = out[8];
        let len = u64::from_le_bytes(out[9..17].try_into().unwrap()) as usize;
        let body: Vec<u8> = match kind {
            COMP_NONE => out[17..].to_vec(),
            _ => decode_zstd(&out[17..], len).unwrap(),
        };
        let (v3, _) = bincode::serde::decode_from_slice::<DimensionEntities, _>(
            &body, cfg,
        )
        .unwrap();
        assert_eq!(v3.format_version, 3);
        assert_eq!(v3.mobs.len(), 1);
        assert_eq!(v3.mobs[0].kind, 11);
        assert!(!v3.mobs[0].persistent);
        assert_eq!(v3.mobs[0].baby_left, 0.0);
        assert!(v3.structure_mob_anchors.is_empty());
    }

    #[test]
    fn empty_constructor_uses_current_format_version() {
        let e = DimensionEntities::empty();
        assert_eq!(e.format_version, ENTITIES_FORMAT_VERSION);
        assert!(e.mobs.is_empty());
        assert!(e.drops.is_empty());
        assert!(e.carts.is_empty());
        assert_eq!(e.next_mob_id, 0);
        assert_eq!(e.next_drop_id, 0);
        assert_eq!(e.next_cart_id, 0);
    }

    /// W137: a hand-built v1 envelope (mobs + drops, no carts) must
    /// migrate to v2 through the production arrow with empty carts.
    #[test]
    fn v1_envelope_migrates_to_v2_with_empty_carts() {
        // Encode a v1 payload using the frozen V1 field order.
        #[derive(Serialize)]
        struct V1Out {
            format_version: u32,
            mobs: Vec<MobSaveV2>,
            drops: Vec<DropSave>,
            next_mob_id: u32,
            next_drop_id: u32,
        }
        let v1 = V1Out {
            format_version: 1,
            mobs: vec![MobSaveV2 {
                id: 7,
                kind: 1,
                pos: [1.0, 2.0, 3.0],
                yaw: 0.5,
                vel: [0.0; 3],
                health: 8.0,
                age_seconds: 0.0,
            }],
            drops: Vec::new(),
            next_mob_id: 8,
            next_drop_id: 1,
        };
        let cfg = bincode::config::standard();
        let raw = bincode::serde::encode_to_vec(&v1, cfg).unwrap();
        let mut file: Vec<u8> = Vec::new();
        file.extend_from_slice(ENTITIES_MAGIC);
        file.extend_from_slice(&1u32.to_le_bytes());
        file.push(COMP_NONE);
        file.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        file.extend_from_slice(&raw);

        let v2_bytes = entities_v1_to_v2(&file).expect("migration");
        // Re-decode through the standard reader path pieces.
        let version =
            u32::from_le_bytes([v2_bytes[4], v2_bytes[5], v2_bytes[6], v2_bytes[7]]);
        assert_eq!(version, 2);
        let kind = v2_bytes[8];
        let len = u64::from_le_bytes([
            v2_bytes[9], v2_bytes[10], v2_bytes[11], v2_bytes[12], v2_bytes[13],
            v2_bytes[14], v2_bytes[15], v2_bytes[16],
        ]) as usize;
        let body = &v2_bytes[17..];
        let raw2 = if kind == COMP_ZSTD {
            decode_zstd(body, len).unwrap()
        } else {
            body.to_vec()
        };
        // W302: the arrow emits the TRUE v2 shape now - decode with
        // the frozen legacy struct (the chained 2 -> 3 arrow owns the
        // lift to the current shape).
        let (v2, _) = bincode::serde::decode_from_slice::<DimensionEntitiesV2, _>(
            &raw2,
            cfg,
        )
        .unwrap();
        assert_eq!(v2.format_version, 2);
        assert_eq!(v2.mobs.len(), 1);
        assert_eq!(v2.mobs[0].id, 7);
        assert!(v2.carts.is_empty());
        assert_eq!(v2.next_cart_id, 0);
        assert_eq!(v2.next_mob_id, 8);
    }
}
