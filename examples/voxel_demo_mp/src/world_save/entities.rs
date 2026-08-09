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
/// - v5 (W459): adds per-mob `trade_xp` (villager levelling
///   survives reload). Migration in `entities_v4_to_v5`.
/// - v6 (W468): `DropSave` carries a full `ItemStack` (count +
///   components) instead of a bare block id - stack sizes and
///   damage/enchants/name survive reload (pre-v6 saves collapsed
///   every drop to count 1 on load). Migration in
///   `entities_v5_to_v6`.
/// - v7 (W474): adds per-mob `sheared` (a shorn sheep stays shorn
///   across reload until it eats grass). Migration in
///   `entities_v6_to_v7` appends `sheared: false`.
/// - v8 (W476.1): adds per-mob `size` (slime / magma-cube 1|2|4 -
///   a saved size-4 slime reloaded at size 1 before). Migration in
///   `entities_v7_to_v8` appends `size: 1`.
/// - v9 (W479): adds per-mob `despawn_delay` (the wandering trader's
///   48000-tick budget; 0 = no timer). Migration in
///   `entities_v9::entities_v8_to_v9` appends `despawn_delay: 0`
///   (the frozen v8 shapes live there too).
pub(crate) const ENTITIES_FORMAT_VERSION: u32 = 9;

/// Default zstd compression level. Matches `chunks.rs` + `players.rs` -
/// level 3 is the documented "balanced" setting from the design doc.
/// W479: `pub(super)` so the `entities_v9` arrow module shares the
/// envelope helpers instead of copying them.
pub(super) const ZSTD_LEVEL: i32 = 3;

/// Compression-kind discriminant byte stored in the envelope.
pub(super) const COMP_NONE: u8 = 0;
pub(super) const COMP_ZSTD: u8 = 1;

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
    /// W348 (v4): MC `AbstractHorse` tame flag.
    pub tamed: bool,
    /// W348 (v4): MC temper 0..100 (the taming ladder).
    pub temper: u8,
    /// W459 (v5): MC VillagerData accumulated trade XP - the
    /// Novice..Master tier unlocks survive reload.
    pub trade_xp: u32,
    /// W474 (v7): MC `Sheep` sheared flag - a shorn sheep reloads
    /// shorn until it eats grass.
    pub sheared: bool,
    /// W476.1 (v8): MC `Slime.ID_SIZE` - 1, 2 or 4 for the slime /
    /// magma-cube family (HP = size^2, render scale = size). Every
    /// other kind stays at 1. The load side clamps to the 1|2|4
    /// domain.
    pub size: u8,
    /// W479 (v9): MC `WanderingTrader.DespawnDelay` - game ticks
    /// left before the mob removes itself (48000 on a fresh trader);
    /// 0 = no timer (every other kind).
    pub despawn_delay: u32,
    // Future fields go here - with a version bump + migration arrow
    // (bincode is positional; `#[serde(default)]` does NOT cover it).
}

/// One persistent dropped-item entry. Despawn timers live on the
/// runtime side; here we just store the age so the dispatcher can
/// re-derive remaining time on reload.
///
/// W468 (v6): carries the runtime's full `ItemStack` (id + count +
/// components) - a merged 30-cobble drop or a worn tool on the
/// ground survives reload intact. Pre-v6 saves stored one bare
/// block id per drop and reloaded everything at count 1.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct DropSave {
    /// Stable per-save drop id. Allocated from
    /// `DimensionEntities::next_drop_id`.
    pub id: u32,
    /// The full stack this drop represents.
    pub stack: crate::item::ItemStack,
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
pub(super) fn decode_zstd(body: &[u8], expected_len: usize) -> io::Result<Vec<u8>> {
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

pub(super) fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

// ---------------------------------------------------------------
// Migration: v1 -> v2 (W137 - adds `carts` + `next_cart_id`).
// ---------------------------------------------------------------

/// W468: the v1..v5 on-disk drop shape (bare block id, no count /
/// components), frozen so every older arrow keeps decoding and
/// re-encoding true legacy bytes while the live `DropSave` carries
/// a full `ItemStack`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct DropSaveV5 {
    id: u32,
    block: u16,
    pos: [f32; 3],
    vel: [f32; 3],
    age: f32,
}

impl DropSaveV5 {
    /// Lift to the FROZEN v6 shape. Legacy saves stored one entry
    /// per drop entity with no count, and the old apply reloaded
    /// each at count 1 with empty components - the lift mirrors
    /// that exactly so a migrated save reloads identically.
    fn lift(self) -> DropSaveV6 {
        DropSaveV6 {
            id: self.id,
            stack: crate::item::ItemStack::new(self.block, 1),
            pos: self.pos,
            vel: self.vel,
            age: self.age,
        }
    }
}

/// Frozen snapshot of the v1 payload shape (mobs + drops only).
/// Kept private to the migration arrow so the live struct can keep
/// evolving without poisoning old decodes.
#[derive(Debug, Deserialize)]
struct DimensionEntitiesV1 {
    #[allow(dead_code)]
    format_version: u32,
    mobs: Vec<MobSaveV2>,
    drops: Vec<DropSaveV5>,
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
    drops: Vec<DropSaveV5>,
    next_mob_id: u32,
    next_drop_id: u32,
    carts: Vec<CartSave>,
    next_cart_id: u32,
}

impl MobSaveV2 {
    /// Lift to the FROZEN v3 shape with the v3 defaults.
    fn lift(self) -> MobSaveV3 {
        MobSaveV3 {
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

/// W348: the v3 on-disk mob shape (pre-`tamed`/`temper`), frozen so
/// the 2 -> 3 and 3 -> 4 arrows keep decoding/encoding true v3 bytes
/// while the live struct evolves.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MobSaveV3 {
    id: u32,
    kind: u8,
    pos: [f32; 3],
    yaw: f32,
    vel: [f32; 3],
    health: f32,
    age_seconds: f32,
    persistent: bool,
    baby_left: f32,
}

/// W348: the v3 on-disk dimension shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct DimensionEntitiesV3 {
    format_version: u32,
    mobs: Vec<MobSaveV3>,
    drops: Vec<DropSaveV5>,
    next_mob_id: u32,
    next_drop_id: u32,
    carts: Vec<CartSave>,
    next_cart_id: u32,
    structure_mob_anchors: Vec<[i32; 3]>,
}

impl MobSaveV3 {
    /// Lift to the FROZEN v4 shape with the v4 defaults.
    fn lift(self) -> MobSaveV4 {
        MobSaveV4 {
            id: self.id,
            kind: self.kind,
            pos: self.pos,
            yaw: self.yaw,
            vel: self.vel,
            health: self.health,
            age_seconds: self.age_seconds,
            persistent: self.persistent,
            baby_left: self.baby_left,
            tamed: false,
            temper: 0,
        }
    }
}

/// W459: the v4 on-disk mob shape (pre-`trade_xp`), frozen so the
/// 3 -> 4 and 4 -> 5 arrows keep decoding/encoding true v4 bytes
/// while the live struct evolves.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MobSaveV4 {
    id: u32,
    kind: u8,
    pos: [f32; 3],
    yaw: f32,
    vel: [f32; 3],
    health: f32,
    age_seconds: f32,
    persistent: bool,
    baby_left: f32,
    tamed: bool,
    temper: u8,
}

/// W459: the v4 on-disk dimension shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct DimensionEntitiesV4 {
    format_version: u32,
    mobs: Vec<MobSaveV4>,
    drops: Vec<DropSaveV5>,
    next_mob_id: u32,
    next_drop_id: u32,
    carts: Vec<CartSave>,
    next_cart_id: u32,
    structure_mob_anchors: Vec<[i32; 3]>,
}

impl MobSaveV4 {
    /// Lift to the FROZEN v5 shape with the v5 defaults.
    fn lift(self) -> MobSaveV5 {
        MobSaveV5 {
            id: self.id,
            kind: self.kind,
            pos: self.pos,
            yaw: self.yaw,
            vel: self.vel,
            health: self.health,
            age_seconds: self.age_seconds,
            persistent: self.persistent,
            baby_left: self.baby_left,
            tamed: self.tamed,
            temper: self.temper,
            trade_xp: 0,
        }
    }
}

/// W468: the v5 on-disk mob shape, frozen even though v6 does not
/// change mobs - future `MobSave` growth must never poison the
/// 4 -> 5 / 5 -> 6 arrows (the W459 entities lesson).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MobSaveV5 {
    id: u32,
    kind: u8,
    pos: [f32; 3],
    yaw: f32,
    vel: [f32; 3],
    health: f32,
    age_seconds: f32,
    persistent: bool,
    baby_left: f32,
    tamed: bool,
    temper: u8,
    trade_xp: u32,
}

impl MobSaveV5 {
    /// Lift to the FROZEN v6 shape (field-identical; the copy keeps
    /// the 5 -> 6 arrow byte-frozen while the live struct evolves).
    fn lift(self) -> MobSaveV6 {
        MobSaveV6 {
            id: self.id,
            kind: self.kind,
            pos: self.pos,
            yaw: self.yaw,
            vel: self.vel,
            health: self.health,
            age_seconds: self.age_seconds,
            persistent: self.persistent,
            baby_left: self.baby_left,
            tamed: self.tamed,
            temper: self.temper,
            trade_xp: self.trade_xp,
        }
    }
}

/// W468: the v5 on-disk dimension shape (tuple-era drops).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct DimensionEntitiesV5 {
    format_version: u32,
    mobs: Vec<MobSaveV5>,
    drops: Vec<DropSaveV5>,
    next_mob_id: u32,
    next_drop_id: u32,
    carts: Vec<CartSave>,
    next_cart_id: u32,
    structure_mob_anchors: Vec<[i32; 3]>,
}

/// W474: the v6 on-disk mob shape (pre-`sheared`), frozen so the
/// 5 -> 6 and 6 -> 7 arrows keep decoding/encoding true v6 bytes
/// while the live `MobSave` evolves.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MobSaveV6 {
    id: u32,
    kind: u8,
    pos: [f32; 3],
    yaw: f32,
    vel: [f32; 3],
    health: f32,
    age_seconds: f32,
    persistent: bool,
    baby_left: f32,
    tamed: bool,
    temper: u8,
    trade_xp: u32,
}

impl MobSaveV6 {
    /// Lift to the FROZEN v7 shape with the v7 default.
    fn lift(self) -> MobSaveV7 {
        MobSaveV7 {
            id: self.id,
            kind: self.kind,
            pos: self.pos,
            yaw: self.yaw,
            vel: self.vel,
            health: self.health,
            age_seconds: self.age_seconds,
            persistent: self.persistent,
            baby_left: self.baby_left,
            tamed: self.tamed,
            temper: self.temper,
            trade_xp: self.trade_xp,
            sheared: false,
        }
    }
}

/// W474: the v6 on-disk drop shape, frozen even though v7 does not
/// change drops - future `DropSave` growth must never poison the
/// 5 -> 6 / 6 -> 7 arrows (the W459/W468 freeze-everything rule).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct DropSaveV6 {
    id: u32,
    stack: crate::item::ItemStack,
    pos: [f32; 3],
    vel: [f32; 3],
    age: f32,
}

impl DropSaveV6 {
    /// Lift to the FROZEN v7 shape (field-identical).
    fn lift(self) -> DropSaveV7 {
        DropSaveV7 {
            id: self.id,
            stack: self.stack,
            pos: self.pos,
            vel: self.vel,
            age: self.age,
        }
    }
}

/// W474: the v6 on-disk dimension shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct DimensionEntitiesV6 {
    format_version: u32,
    mobs: Vec<MobSaveV6>,
    drops: Vec<DropSaveV6>,
    next_mob_id: u32,
    next_drop_id: u32,
    carts: Vec<CartSave>,
    next_cart_id: u32,
    structure_mob_anchors: Vec<[i32; 3]>,
}

/// W476.1: the v7 on-disk mob shape (pre-`size`), frozen so the
/// 6 -> 7 and 7 -> 8 arrows keep decoding/encoding true v7 bytes
/// while the live `MobSave` evolves.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MobSaveV7 {
    id: u32,
    kind: u8,
    pos: [f32; 3],
    yaw: f32,
    vel: [f32; 3],
    health: f32,
    age_seconds: f32,
    persistent: bool,
    baby_left: f32,
    tamed: bool,
    temper: u8,
    trade_xp: u32,
    sheared: bool,
}

impl MobSaveV7 {
    /// Lift to the FROZEN v8 shape with the v8 default (W479: the
    /// live struct is v9 now - encoding it under a v8 tag would
    /// break the chained 8 -> 9 arrow). Pre-v8 saves predate
    /// slime-size persistence; size-1 is exactly what the old load
    /// path reconstructed for every mob.
    fn lift(self) -> super::entities_v9::MobSaveV8 {
        super::entities_v9::MobSaveV8 {
            id: self.id,
            kind: self.kind,
            pos: self.pos,
            yaw: self.yaw,
            vel: self.vel,
            health: self.health,
            age_seconds: self.age_seconds,
            persistent: self.persistent,
            baby_left: self.baby_left,
            tamed: self.tamed,
            temper: self.temper,
            trade_xp: self.trade_xp,
            sheared: self.sheared,
            size: 1,
        }
    }
}

/// W476.1: the v7 on-disk drop shape, frozen even though v8 does
/// not change drops (the W459/W468 freeze-everything rule).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct DropSaveV7 {
    id: u32,
    stack: crate::item::ItemStack,
    pos: [f32; 3],
    vel: [f32; 3],
    age: f32,
}

impl DropSaveV7 {
    /// Lift to the FROZEN v8 shape (field-identical; W479 freeze).
    fn lift(self) -> super::entities_v9::DropSaveV8 {
        super::entities_v9::DropSaveV8 {
            id: self.id,
            stack: self.stack,
            pos: self.pos,
            vel: self.vel,
            age: self.age,
        }
    }
}

/// W476.1: the v7 on-disk dimension shape.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct DimensionEntitiesV7 {
    format_version: u32,
    mobs: Vec<MobSaveV7>,
    drops: Vec<DropSaveV7>,
    next_mob_id: u32,
    next_drop_id: u32,
    carts: Vec<CartSave>,
    next_cart_id: u32,
    structure_mob_anchors: Vec<[i32; 3]>,
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
    // W348: emit the TRUE (frozen) v3 shape - encoding the current
    // (v4) struct under a v3 tag would break the chained 3 -> 4
    // arrow, exactly like the v1 -> v2 precedent.
    let v3 = DimensionEntitiesV3 {
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

/// W348: one-step v3 -> v4 migration arrow. Decodes the frozen v3
/// shape and lifts every mob with `tamed: false` / `temper: 0`
/// (horse taming persistence). Same envelope walk as its siblings.
pub(crate) fn entities_v3_to_v4(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() < 17 {
        return Err(invalid("entities v3 envelope truncated"));
    }
    if &bytes[..4] != ENTITIES_MAGIC {
        return Err(invalid("entities v3 envelope magic mismatch"));
    }
    let on_disk_v = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if on_disk_v != 3 {
        return Err(invalid(format!(
            "entities_v3_to_v4 expected v3, got v{on_disk_v}"
        )));
    }
    let kind = bytes[8];
    let uncompressed_len = u64::from_le_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15], bytes[16],
    ]) as usize;
    let body = &bytes[17..];
    let raw_v3: Vec<u8> = match kind {
        COMP_NONE => {
            if body.len() != uncompressed_len {
                return Err(invalid("entities v3 body length mismatch during migration"));
            }
            body.to_vec()
        }
        COMP_ZSTD => decode_zstd(body, uncompressed_len)?,
        other => {
            return Err(invalid(format!(
                "unknown entities v3 compression kind {other} during migration"
            )));
        }
    };
    let cfg = bincode::config::standard();
    let (v3, _read) = bincode::serde::decode_from_slice::<DimensionEntitiesV3, _>(&raw_v3, cfg)
        .map_err(|err| invalid(format!("entities v3 payload decode failed: {err}")))?;
    // W459: emit the FROZEN v4 shape (the live struct is v5 now).
    let v4 = DimensionEntitiesV4 {
        format_version: 4,
        mobs: v3.mobs.into_iter().map(MobSaveV3::lift).collect(),
        drops: v3.drops,
        next_mob_id: v3.next_mob_id,
        next_drop_id: v3.next_drop_id,
        carts: v3.carts,
        next_cart_id: v3.next_cart_id,
        structure_mob_anchors: v3.structure_mob_anchors,
    };
    let raw_v4 = bincode::serde::encode_to_vec(&v4, cfg)
        .map_err(|err| invalid(format!("entities v4 payload encode failed: {err}")))?;
    let uncompressed_len_v4 = raw_v4.len() as u64;
    let (kind_v4, body_v4): (u8, Vec<u8>) = match zstd::encode_all(&raw_v4[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw_v4.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw_v4),
    };
    let mut out: Vec<u8> = Vec::with_capacity(body_v4.len() + 17);
    out.extend_from_slice(ENTITIES_MAGIC);
    out.extend_from_slice(&4u32.to_le_bytes());
    out.push(kind_v4);
    out.extend_from_slice(&uncompressed_len_v4.to_le_bytes());
    out.extend_from_slice(&body_v4);
    Ok(out)
}

/// W459: one-step v4 -> v5 migration arrow - lifts every mob with
/// `trade_xp: 0`. Same envelope handling as the earlier arrows.
pub(crate) fn entities_v4_to_v5(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() < 17 {
        return Err(invalid("entities v4 envelope truncated"));
    }
    if &bytes[..4] != ENTITIES_MAGIC {
        return Err(invalid("entities v4 envelope magic mismatch"));
    }
    let on_disk_v = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if on_disk_v != 4 {
        return Err(invalid(format!(
            "entities_v4_to_v5 expected v4, got v{on_disk_v}"
        )));
    }
    let kind = bytes[8];
    let uncompressed_len = u64::from_le_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15], bytes[16],
    ]) as usize;
    let body = &bytes[17..];
    let raw_v4: Vec<u8> = match kind {
        COMP_NONE => {
            if body.len() != uncompressed_len {
                return Err(invalid("entities v4 body length mismatch during migration"));
            }
            body.to_vec()
        }
        COMP_ZSTD => decode_zstd(body, uncompressed_len)?,
        other => {
            return Err(invalid(format!(
                "unknown entities v4 compression kind {other} during migration"
            )));
        }
    };
    let cfg = bincode::config::standard();
    let (v4, _read) = bincode::serde::decode_from_slice::<DimensionEntitiesV4, _>(&raw_v4, cfg)
        .map_err(|err| invalid(format!("entities v4 payload decode failed: {err}")))?;
    // W468: emit the FROZEN v5 shape (the live struct is v6 now).
    let v5 = DimensionEntitiesV5 {
        format_version: 5,
        mobs: v4.mobs.into_iter().map(MobSaveV4::lift).collect(),
        drops: v4.drops,
        next_mob_id: v4.next_mob_id,
        next_drop_id: v4.next_drop_id,
        carts: v4.carts,
        next_cart_id: v4.next_cart_id,
        structure_mob_anchors: v4.structure_mob_anchors,
    };
    let raw_v5 = bincode::serde::encode_to_vec(&v5, cfg)
        .map_err(|err| invalid(format!("entities v5 payload encode failed: {err}")))?;
    let uncompressed_len_v5 = raw_v5.len() as u64;
    let (kind_v5, body_v5): (u8, Vec<u8>) = match zstd::encode_all(&raw_v5[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw_v5.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw_v5),
    };
    let mut out: Vec<u8> = Vec::with_capacity(body_v5.len() + 17);
    out.extend_from_slice(ENTITIES_MAGIC);
    out.extend_from_slice(&5u32.to_le_bytes());
    out.push(kind_v5);
    out.extend_from_slice(&uncompressed_len_v5.to_le_bytes());
    out.extend_from_slice(&body_v5);
    Ok(out)
}

/// W468: one-step v5 -> v6 migration arrow - lifts every drop's
/// bare block id into an `ItemStack` with count 1 and empty
/// components (exactly what the old load path reconstructed), and
/// copies mobs across the frozen v5 shape unchanged. Same envelope
/// handling as the earlier arrows.
pub(crate) fn entities_v5_to_v6(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() < 17 {
        return Err(invalid("entities v5 envelope truncated"));
    }
    if &bytes[..4] != ENTITIES_MAGIC {
        return Err(invalid("entities v5 envelope magic mismatch"));
    }
    let on_disk_v = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if on_disk_v != 5 {
        return Err(invalid(format!(
            "entities_v5_to_v6 expected v5, got v{on_disk_v}"
        )));
    }
    let kind = bytes[8];
    let uncompressed_len = u64::from_le_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15], bytes[16],
    ]) as usize;
    let body = &bytes[17..];
    let raw_v5: Vec<u8> = match kind {
        COMP_NONE => {
            if body.len() != uncompressed_len {
                return Err(invalid("entities v5 body length mismatch during migration"));
            }
            body.to_vec()
        }
        COMP_ZSTD => decode_zstd(body, uncompressed_len)?,
        other => {
            return Err(invalid(format!(
                "unknown entities v5 compression kind {other} during migration"
            )));
        }
    };
    let cfg = bincode::config::standard();
    let (v5, _read) = bincode::serde::decode_from_slice::<DimensionEntitiesV5, _>(&raw_v5, cfg)
        .map_err(|err| invalid(format!("entities v5 payload decode failed: {err}")))?;
    // W474: emit the FROZEN v6 shape (the live struct is v7 now) -
    // encoding the current struct under a v6 tag would break the
    // chained 6 -> 7 arrow, exactly like the earlier precedents.
    let v6 = DimensionEntitiesV6 {
        format_version: 6,
        mobs: v5.mobs.into_iter().map(MobSaveV5::lift).collect(),
        drops: v5.drops.into_iter().map(DropSaveV5::lift).collect(),
        next_mob_id: v5.next_mob_id,
        next_drop_id: v5.next_drop_id,
        carts: v5.carts,
        next_cart_id: v5.next_cart_id,
        structure_mob_anchors: v5.structure_mob_anchors,
    };
    let raw_v6 = bincode::serde::encode_to_vec(&v6, cfg)
        .map_err(|err| invalid(format!("entities v6 payload encode failed: {err}")))?;
    let uncompressed_len_v6 = raw_v6.len() as u64;
    let (kind_v6, body_v6): (u8, Vec<u8>) = match zstd::encode_all(&raw_v6[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw_v6.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw_v6),
    };
    let mut out: Vec<u8> = Vec::with_capacity(body_v6.len() + 17);
    out.extend_from_slice(ENTITIES_MAGIC);
    out.extend_from_slice(&6u32.to_le_bytes());
    out.push(kind_v6);
    out.extend_from_slice(&uncompressed_len_v6.to_le_bytes());
    out.extend_from_slice(&body_v6);
    Ok(out)
}

/// W474: one-step v6 -> v7 migration arrow - lifts every mob with
/// `sheared: false` (pre-v7 saves predate shearing) and copies the
/// frozen v6 drops across unchanged. Same envelope handling as the
/// earlier arrows.
pub(crate) fn entities_v6_to_v7(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() < 17 {
        return Err(invalid("entities v6 envelope truncated"));
    }
    if &bytes[..4] != ENTITIES_MAGIC {
        return Err(invalid("entities v6 envelope magic mismatch"));
    }
    let on_disk_v = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if on_disk_v != 6 {
        return Err(invalid(format!(
            "entities_v6_to_v7 expected v6, got v{on_disk_v}"
        )));
    }
    let kind = bytes[8];
    let uncompressed_len = u64::from_le_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15], bytes[16],
    ]) as usize;
    let body = &bytes[17..];
    let raw_v6: Vec<u8> = match kind {
        COMP_NONE => {
            if body.len() != uncompressed_len {
                return Err(invalid("entities v6 body length mismatch during migration"));
            }
            body.to_vec()
        }
        COMP_ZSTD => decode_zstd(body, uncompressed_len)?,
        other => {
            return Err(invalid(format!(
                "unknown entities v6 compression kind {other} during migration"
            )));
        }
    };
    let cfg = bincode::config::standard();
    let (v6, _read) = bincode::serde::decode_from_slice::<DimensionEntitiesV6, _>(&raw_v6, cfg)
        .map_err(|err| invalid(format!("entities v6 payload decode failed: {err}")))?;
    // W476.1: emit the FROZEN v7 shape (the live struct is v8 now) -
    // encoding the current struct under a v7 tag would break the
    // chained 7 -> 8 arrow, exactly like the earlier precedents.
    let v7 = DimensionEntitiesV7 {
        format_version: 7,
        mobs: v6.mobs.into_iter().map(MobSaveV6::lift).collect(),
        drops: v6.drops.into_iter().map(DropSaveV6::lift).collect(),
        next_mob_id: v6.next_mob_id,
        next_drop_id: v6.next_drop_id,
        carts: v6.carts,
        next_cart_id: v6.next_cart_id,
        structure_mob_anchors: v6.structure_mob_anchors,
    };
    let raw_v7 = bincode::serde::encode_to_vec(&v7, cfg)
        .map_err(|err| invalid(format!("entities v7 payload encode failed: {err}")))?;
    let uncompressed_len_v7 = raw_v7.len() as u64;
    let (kind_v7, body_v7): (u8, Vec<u8>) = match zstd::encode_all(&raw_v7[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw_v7.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw_v7),
    };
    let mut out: Vec<u8> = Vec::with_capacity(body_v7.len() + 17);
    out.extend_from_slice(ENTITIES_MAGIC);
    out.extend_from_slice(&7u32.to_le_bytes());
    out.push(kind_v7);
    out.extend_from_slice(&uncompressed_len_v7.to_le_bytes());
    out.extend_from_slice(&body_v7);
    Ok(out)
}

/// W476.1: one-step v7 -> v8 migration arrow - lifts every mob with
/// `size: 1` (pre-v8 saves reloaded every slime at size 1; the lift
/// preserves that exact behaviour) and copies the frozen v7 drops
/// across unchanged. Same envelope handling as the earlier arrows.
pub(crate) fn entities_v7_to_v8(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() < 17 {
        return Err(invalid("entities v7 envelope truncated"));
    }
    if &bytes[..4] != ENTITIES_MAGIC {
        return Err(invalid("entities v7 envelope magic mismatch"));
    }
    let on_disk_v = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if on_disk_v != 7 {
        return Err(invalid(format!(
            "entities_v7_to_v8 expected v7, got v{on_disk_v}"
        )));
    }
    let kind = bytes[8];
    let uncompressed_len = u64::from_le_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15], bytes[16],
    ]) as usize;
    let body = &bytes[17..];
    let raw_v7: Vec<u8> = match kind {
        COMP_NONE => {
            if body.len() != uncompressed_len {
                return Err(invalid("entities v7 body length mismatch during migration"));
            }
            body.to_vec()
        }
        COMP_ZSTD => decode_zstd(body, uncompressed_len)?,
        other => {
            return Err(invalid(format!(
                "unknown entities v7 compression kind {other} during migration"
            )));
        }
    };
    let cfg = bincode::config::standard();
    let (v7, _read) = bincode::serde::decode_from_slice::<DimensionEntitiesV7, _>(&raw_v7, cfg)
        .map_err(|err| invalid(format!("entities v7 payload decode failed: {err}")))?;
    // W479: emit the FROZEN v8 shape (the live struct is v9 now).
    let v8 = super::entities_v9::DimensionEntitiesV8 {
        format_version: 8,
        mobs: v7.mobs.into_iter().map(MobSaveV7::lift).collect(),
        drops: v7.drops.into_iter().map(DropSaveV7::lift).collect(),
        next_mob_id: v7.next_mob_id,
        next_drop_id: v7.next_drop_id,
        carts: v7.carts,
        next_cart_id: v7.next_cart_id,
        structure_mob_anchors: v7.structure_mob_anchors,
    };
    let raw_v8 = bincode::serde::encode_to_vec(&v8, cfg)
        .map_err(|err| invalid(format!("entities v8 payload encode failed: {err}")))?;
    let uncompressed_len_v8 = raw_v8.len() as u64;
    let (kind_v8, body_v8): (u8, Vec<u8>) = match zstd::encode_all(&raw_v8[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw_v8.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw_v8),
    };
    let mut out: Vec<u8> = Vec::with_capacity(body_v8.len() + 17);
    out.extend_from_slice(ENTITIES_MAGIC);
    out.extend_from_slice(&8u32.to_le_bytes());
    out.push(kind_v8);
    out.extend_from_slice(&uncompressed_len_v8.to_le_bytes());
    out.extend_from_slice(&body_v8);
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
        // W348: the arrow emits the FROZEN v3 shape now.
        let (v3, _) = bincode::serde::decode_from_slice::<DimensionEntitiesV3, _>(
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

    /// W348: a hand-built v3 envelope lifts through the 3 -> 4
    /// arrow with tamed=false / temper=0 on every mob.
    #[test]
    fn entities_v3_to_v4_lifts_tame_fields() {
        let cfg = bincode::config::standard();
        let v3 = DimensionEntitiesV3 {
            format_version: 3,
            mobs: vec![MobSaveV3 {
                id: 9,
                kind: 14,
                pos: [1.0, 70.0, -2.0],
                yaw: 0.0,
                vel: [0.0; 3],
                health: 22.0,
                age_seconds: 0.0,
                persistent: true,
                baby_left: 0.0,
            }],
            drops: Vec::new(),
            next_mob_id: 10,
            next_drop_id: 1,
            carts: Vec::new(),
            next_cart_id: 1,
            structure_mob_anchors: vec![[1, 2, 3]],
        };
        let raw = bincode::serde::encode_to_vec(&v3, cfg).unwrap();
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(ENTITIES_MAGIC);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.push(COMP_NONE);
        bytes.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&raw);
        let out = entities_v3_to_v4(&bytes).expect("arrow lifts");
        assert_eq!(u32::from_le_bytes([out[4], out[5], out[6], out[7]]), 4);
        let kind = out[8];
        let len = u64::from_le_bytes(out[9..17].try_into().unwrap()) as usize;
        let body: Vec<u8> = match kind {
            COMP_NONE => out[17..].to_vec(),
            _ => decode_zstd(&out[17..], len).unwrap(),
        };
        // W459: the arrow emits the FROZEN v4 shape now.
        let (v4, _) = bincode::serde::decode_from_slice::<DimensionEntitiesV4, _>(
            &body, cfg,
        )
        .unwrap();
        assert_eq!(v4.format_version, 4);
        assert_eq!(v4.mobs.len(), 1);
        assert!(!v4.mobs[0].tamed);
        assert_eq!(v4.mobs[0].temper, 0);
        assert!(v4.mobs[0].persistent, "v3 fields carry through");
        assert_eq!(v4.structure_mob_anchors, vec![[1, 2, 3]]);
    }

    /// W459: a hand-built v4 envelope lifts through the 4 -> 5
    /// arrow with trade_xp=0 on every mob; v4 fields carry through.
    #[test]
    fn entities_v4_to_v5_lifts_trade_xp() {
        let cfg = bincode::config::standard();
        let v4 = DimensionEntitiesV4 {
            format_version: 4,
            mobs: vec![MobSaveV4 {
                id: 11,
                kind: 11,
                pos: [4.0, 65.0, 4.0],
                yaw: 0.0,
                vel: [0.0; 3],
                health: 20.0,
                age_seconds: 0.0,
                persistent: true,
                baby_left: 0.0,
                tamed: true,
                temper: 40,
            }],
            drops: Vec::new(),
            next_mob_id: 12,
            next_drop_id: 1,
            carts: Vec::new(),
            next_cart_id: 1,
            structure_mob_anchors: vec![[7, 8, 9]],
        };
        let raw = bincode::serde::encode_to_vec(&v4, cfg).unwrap();
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(ENTITIES_MAGIC);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.push(COMP_NONE);
        bytes.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&raw);
        let out = entities_v4_to_v5(&bytes).expect("arrow lifts");
        assert_eq!(u32::from_le_bytes([out[4], out[5], out[6], out[7]]), 5);
        let kind = out[8];
        let len = u64::from_le_bytes(out[9..17].try_into().unwrap()) as usize;
        let body: Vec<u8> = match kind {
            COMP_NONE => out[17..].to_vec(),
            _ => decode_zstd(&out[17..], len).unwrap(),
        };
        // W468: the arrow emits the FROZEN v5 shape now.
        let (v5, _) = bincode::serde::decode_from_slice::<DimensionEntitiesV5, _>(
            &body, cfg,
        )
        .unwrap();
        assert_eq!(v5.format_version, 5);
        assert_eq!(v5.mobs.len(), 1);
        assert_eq!(v5.mobs[0].trade_xp, 0);
        assert!(v5.mobs[0].tamed, "v4 fields carry through");
        assert_eq!(v5.mobs[0].temper, 40);
        assert_eq!(v5.structure_mob_anchors, vec![[7, 8, 9]]);
    }

    /// W468: a hand-built FROZEN v5 envelope lifts through the
    /// 5 -> 6 arrow - drops gain `ItemStack::new(block, 1)` (the
    /// exact shape the old load path reconstructed), mob fields
    /// (trade_xp included) carry through untouched.
    #[test]
    fn entities_v5_to_v6_lifts_drops_to_stacks() {
        let cfg = bincode::config::standard();
        let v5 = DimensionEntitiesV5 {
            format_version: 5,
            mobs: vec![MobSaveV5 {
                id: 3,
                kind: 11,
                pos: [1.0, 64.0, 1.0],
                yaw: 0.0,
                vel: [0.0; 3],
                health: 20.0,
                age_seconds: 0.0,
                persistent: true,
                baby_left: 0.0,
                tamed: false,
                temper: 0,
                trade_xp: 77,
            }],
            drops: vec![DropSaveV5 {
                id: 9,
                block: 17,
                pos: [2.5, 65.0, 2.5],
                vel: [0.0, 0.1, 0.0],
                age: 4.0,
            }],
            next_mob_id: 4,
            next_drop_id: 10,
            carts: Vec::new(),
            next_cart_id: 1,
            structure_mob_anchors: vec![[4, 5, 6]],
        };
        let raw = bincode::serde::encode_to_vec(&v5, cfg).unwrap();
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(ENTITIES_MAGIC);
        bytes.extend_from_slice(&5u32.to_le_bytes());
        bytes.push(COMP_NONE);
        bytes.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&raw);
        let out = entities_v5_to_v6(&bytes).expect("arrow lifts");
        assert_eq!(u32::from_le_bytes([out[4], out[5], out[6], out[7]]), 6);
        let kind = out[8];
        let len = u64::from_le_bytes(out[9..17].try_into().unwrap()) as usize;
        let body: Vec<u8> = match kind {
            COMP_NONE => out[17..].to_vec(),
            _ => decode_zstd(&out[17..], len).unwrap(),
        };
        // W474: the arrow emits the FROZEN v6 shape now.
        let (v6, _) = bincode::serde::decode_from_slice::<DimensionEntitiesV6, _>(
            &body, cfg,
        )
        .unwrap();
        assert_eq!(v6.format_version, 6);
        assert_eq!(v6.drops.len(), 1);
        assert_eq!(v6.drops[0].id, 9);
        assert_eq!(v6.drops[0].stack, crate::item::ItemStack::new(17, 1));
        assert_eq!(v6.drops[0].age, 4.0);
        assert_eq!(v6.mobs.len(), 1);
        assert_eq!(v6.mobs[0].trade_xp, 77, "v5 mob fields carry through");
        assert_eq!(v6.structure_mob_anchors, vec![[4, 5, 6]]);
        assert_eq!(v6.next_drop_id, 10);
    }

    /// W474: a hand-built FROZEN v6 envelope lifts through the
    /// 6 -> 7 arrow - every mob gains `sheared: false`, drops and
    /// the rest carry through untouched.
    #[test]
    fn entities_v6_to_v7_lifts_sheared() {
        let cfg = bincode::config::standard();
        let mut worn = crate::item::ItemStack::new(48, 5);
        worn.components.damage = 20;
        let v6 = DimensionEntitiesV6 {
            format_version: 6,
            mobs: vec![MobSaveV6 {
                id: 3,
                kind: 0,
                pos: [1.0, 64.0, 1.0],
                yaw: 0.25,
                vel: [0.0; 3],
                health: 8.0,
                age_seconds: 0.0,
                persistent: true,
                baby_left: 0.0,
                tamed: false,
                temper: 0,
                trade_xp: 12,
            }],
            drops: vec![DropSaveV6 {
                id: 9,
                stack: worn.clone(),
                pos: [2.5, 65.0, 2.5],
                vel: [0.0, 0.1, 0.0],
                age: 4.0,
            }],
            next_mob_id: 4,
            next_drop_id: 10,
            carts: Vec::new(),
            next_cart_id: 1,
            structure_mob_anchors: vec![[4, 5, 6]],
        };
        let raw = bincode::serde::encode_to_vec(&v6, cfg).unwrap();
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(ENTITIES_MAGIC);
        bytes.extend_from_slice(&6u32.to_le_bytes());
        bytes.push(COMP_NONE);
        bytes.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&raw);
        let out = entities_v6_to_v7(&bytes).expect("arrow lifts");
        assert_eq!(u32::from_le_bytes([out[4], out[5], out[6], out[7]]), 7);
        let kind = out[8];
        let len = u64::from_le_bytes(out[9..17].try_into().unwrap()) as usize;
        let body: Vec<u8> = match kind {
            COMP_NONE => out[17..].to_vec(),
            _ => decode_zstd(&out[17..], len).unwrap(),
        };
        // W476.1: the arrow emits the FROZEN v7 shape now.
        let (v7, _) = bincode::serde::decode_from_slice::<DimensionEntitiesV7, _>(
            &body, cfg,
        )
        .unwrap();
        assert_eq!(v7.format_version, 7);
        assert_eq!(v7.mobs.len(), 1);
        assert!(!v7.mobs[0].sheared, "v6 sheep lift unshorn");
        assert_eq!(v7.mobs[0].trade_xp, 12, "v6 mob fields carry through");
        assert_eq!(v7.drops.len(), 1);
        assert_eq!(v7.drops[0].stack, worn, "v6 drop stacks carry through");
        assert_eq!(v7.structure_mob_anchors, vec![[4, 5, 6]]);
        assert_eq!(v7.next_drop_id, 10);
    }

    /// W476.1: a hand-built FROZEN v7 envelope lifts through the
    /// 7 -> 8 arrow - every mob gains `size: 1`, the other fields
    /// and the drops carry through untouched.
    #[test]
    fn entities_v7_to_v8_lifts_size() {
        let cfg = bincode::config::standard();
        let mut worn = crate::item::ItemStack::new(48, 5);
        worn.components.damage = 20;
        let v7 = DimensionEntitiesV7 {
            format_version: 7,
            mobs: vec![MobSaveV7 {
                id: 3,
                kind: 0,
                pos: [1.0, 64.0, 1.0],
                yaw: 0.25,
                vel: [0.0; 3],
                health: 8.0,
                age_seconds: 0.0,
                persistent: true,
                baby_left: 0.0,
                tamed: false,
                temper: 0,
                trade_xp: 12,
                sheared: true,
            }],
            drops: vec![DropSaveV7 {
                id: 9,
                stack: worn.clone(),
                pos: [2.5, 65.0, 2.5],
                vel: [0.0, 0.1, 0.0],
                age: 4.0,
            }],
            next_mob_id: 4,
            next_drop_id: 10,
            carts: Vec::new(),
            next_cart_id: 1,
            structure_mob_anchors: vec![[4, 5, 6]],
        };
        let raw = bincode::serde::encode_to_vec(&v7, cfg).unwrap();
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(ENTITIES_MAGIC);
        bytes.extend_from_slice(&7u32.to_le_bytes());
        bytes.push(COMP_NONE);
        bytes.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&raw);
        let out = entities_v7_to_v8(&bytes).expect("arrow lifts");
        assert_eq!(u32::from_le_bytes([out[4], out[5], out[6], out[7]]), 8);
        let kind = out[8];
        let len = u64::from_le_bytes(out[9..17].try_into().unwrap()) as usize;
        let body: Vec<u8> = match kind {
            COMP_NONE => out[17..].to_vec(),
            _ => decode_zstd(&out[17..], len).unwrap(),
        };
        // W479: the arrow emits the FROZEN v8 shape now.
        let (v8, _) = bincode::serde::decode_from_slice::<
            crate::world_save::entities_v9::DimensionEntitiesV8,
            _,
        >(&body, cfg)
        .unwrap();
        assert_eq!(v8.format_version, 8);
        assert_eq!(v8.mobs.len(), 1);
        assert_eq!(v8.mobs[0].size, 1, "v7 mobs lift at size 1");
        assert!(v8.mobs[0].sheared, "v7 sheared carries through");
        assert_eq!(v8.mobs[0].trade_xp, 12, "v7 mob fields carry through");
        assert_eq!(v8.drops.len(), 1);
        assert_eq!(v8.drops[0].stack, worn, "v7 drop stacks carry through");
        assert_eq!(v8.structure_mob_anchors, vec![[4, 5, 6]]);
        assert_eq!(v8.next_drop_id, 10);
    }

    /// W476.1: the 7 -> 8 arrow refuses input tagged with any other
    /// envelope version.
    #[test]
    fn entities_v7_to_v8_rejects_wrong_version() {
        let cfg = bincode::config::standard();
        let v7 = DimensionEntitiesV7 {
            format_version: 7,
            mobs: Vec::new(),
            drops: Vec::new(),
            next_mob_id: 0,
            next_drop_id: 0,
            carts: Vec::new(),
            next_cart_id: 0,
            structure_mob_anchors: Vec::new(),
        };
        let raw = bincode::serde::encode_to_vec(&v7, cfg).unwrap();
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(ENTITIES_MAGIC);
        bytes.extend_from_slice(&6u32.to_le_bytes());
        bytes.push(COMP_NONE);
        bytes.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&raw);
        let err = entities_v7_to_v8(&bytes).expect_err("must refuse v6 input");
        assert!(
            err.to_string().contains("expected v7"),
            "error names the version mismatch, got: {err}"
        );
    }

    /// W474: the 6 -> 7 arrow refuses input tagged with any other
    /// envelope version.
    #[test]
    fn entities_v6_to_v7_rejects_wrong_version() {
        let cfg = bincode::config::standard();
        let v6 = DimensionEntitiesV6 {
            format_version: 6,
            mobs: Vec::new(),
            drops: Vec::new(),
            next_mob_id: 0,
            next_drop_id: 0,
            carts: Vec::new(),
            next_cart_id: 0,
            structure_mob_anchors: Vec::new(),
        };
        let raw = bincode::serde::encode_to_vec(&v6, cfg).unwrap();
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(ENTITIES_MAGIC);
        bytes.extend_from_slice(&5u32.to_le_bytes());
        bytes.push(COMP_NONE);
        bytes.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&raw);
        let err = entities_v6_to_v7(&bytes).expect_err("must refuse v5 input");
        assert!(
            err.to_string().contains("expected v6"),
            "error names the version mismatch, got: {err}"
        );
    }

    /// W479: the full chain - a legacy v2 envelope walks 2 -> 3 ->
    /// 4 -> 5 -> 6 -> 7 -> 8 -> 9 through the production registry
    /// and decodes at the current version with every appended
    /// default in place.
    #[test]
    fn entities_v2_chains_to_current_through_registry() {
        let cfg = bincode::config::standard();
        let v2 = DimensionEntitiesV2 {
            format_version: 2,
            mobs: vec![MobSaveV2 {
                id: 7,
                kind: 0,
                pos: [1.0, 2.0, 3.0],
                yaw: 0.5,
                vel: [0.0; 3],
                health: 8.0,
                age_seconds: 0.0,
            }],
            drops: vec![DropSaveV5 {
                id: 2,
                block: 17,
                pos: [0.5, 64.0, 0.5],
                vel: [0.0; 3],
                age: 1.0,
            }],
            next_mob_id: 8,
            next_drop_id: 3,
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
        let migrated = migrate::migrate_to(
            migrate::default_registry(),
            SaveKind::Entities,
            ENTITIES_FORMAT_VERSION,
            Path::new("chain-test"),
            2,
            &bytes,
        )
        .expect("registry chains 2 -> current");
        let (payload, _migrated_flag, _post) = decode_entities_bytes(
            &migrated,
            Path::new("chain-test"),
            migrate::default_registry(),
        )
        .expect("current decoder accepts the chained output");
        assert_eq!(payload.format_version, ENTITIES_FORMAT_VERSION);
        assert_eq!(payload.mobs.len(), 1);
        let m = &payload.mobs[0];
        assert!(!m.persistent, "v3 default");
        assert!(!m.tamed, "v4 default");
        assert_eq!(m.trade_xp, 0, "v5 default");
        assert!(!m.sheared, "v7 default");
        assert_eq!(m.size, 1, "v8 default");
        assert_eq!(m.despawn_delay, 0, "v9 default");
        assert_eq!(payload.drops.len(), 1);
        assert_eq!(
            payload.drops[0].stack,
            crate::item::ItemStack::new(17, 1),
            "v6 stack lift"
        );
    }

    /// W468: the 5 -> 6 arrow refuses input tagged with any other
    /// envelope version.
    #[test]
    fn entities_v5_to_v6_rejects_wrong_version() {
        let cfg = bincode::config::standard();
        let v5 = DimensionEntitiesV5 {
            format_version: 5,
            mobs: Vec::new(),
            drops: Vec::new(),
            next_mob_id: 0,
            next_drop_id: 0,
            carts: Vec::new(),
            next_cart_id: 0,
            structure_mob_anchors: Vec::new(),
        };
        let raw = bincode::serde::encode_to_vec(&v5, cfg).unwrap();
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(ENTITIES_MAGIC);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.push(COMP_NONE);
        bytes.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&raw);
        let err = entities_v5_to_v6(&bytes).expect_err("must refuse v4 input");
        assert!(
            err.to_string().contains("expected v5"),
            "error names the version mismatch, got: {err}"
        );
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
            drops: Vec<DropSaveV5>,
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
