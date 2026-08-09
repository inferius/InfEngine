//! Per-player persistent state: position, inventory, vitals, perms.
//!
//! See `docs/design/save_architecture_final.md` ("players/<name>.bin")
//! for the canonical field list. Wire summary:
//!
//! ```text
//! [u8;  4]   magic = b"INFP"
//! [u32]      format_version  (little-endian)
//! [u8]       compression_kind (0 = none, 1 = zstd)
//! [u64]      uncompressed_payload_len (little-endian)
//! [bytes]    payload (bincode-serialised PlayerSave, optionally
//!            zstd-compressed)
//! ```
//!
//! Same envelope shape as `chunks.rs` - one decoder per file
//! family. A player file is ~1 KB so the "try zstd, fall back to
//! raw if it grew" policy still applies; large modded inventories
//! lean on the compressor.
//!
//! `BlockId` is intentionally stored as raw `u16` on the wire +
//! disk: keeps the schema free of the `infengine_voxel` crate's
//! type registry so a server-side ID renumber would not blow up
//! existing player files. The caller maps `u16 <-> BlockId` at the
//! edge of the world-save layer.
//!
//! Phase B.7 is what wires this into the live save / load path.
//! Right now the module is exercised purely by the unit tests in
//! `world_save/players_tests.rs`.
#![allow(dead_code)]

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::atomic::{atomic_save, read_with_recovery};
use super::layout::{world_dir, world_dir_in};
use super::migrate::{self, MigrationRegistry, SaveKind};

/// Magic bytes at the head of every player file: `"INFP"` (InfEngine
/// player). Distinguishes the format from chunks (`INFC`) + the
/// legacy `INFV` state blob.
pub(crate) const PLAYER_MAGIC: &[u8; 4] = b"INFP";

/// Envelope + payload format version. Bumped when the on-disk shape
/// changes in a breaking way.
///
/// History:
/// - v1: Phase B.6 / B.7 initial schema.
/// - v2: Q.2 - adds `highlight_mode: u8` (Auto/Build/Break wireframe
///   selection). Old v1 saves migrate by appending a single `0`
///   byte (= `HighlightMode::Auto`).
/// - v3: W8.2 - adds `game_mode: u8` (Survival / Creative; Hardcore
///   deferred). Old v2 saves migrate by appending a single `0`
///   byte (= `GameMode::Survival`).
/// - v4: W416/W418/W419 - adds `food`/`saturation` (hunger), `xp`,
///   `armor`. Old v3 saves migrate to the MC spawn state.
/// - v5: W466 - inventory slots become full `ItemStack`s (id +
///   count + components with damage/enchants/name) instead of
///   `(u16, u8)` tuples. Old v4 saves lift every slot with empty
///   components in `player_v4_to_v5`.
/// - v6: W469 - `armor` + `craft_grid` become `ItemStack` slots
///   too (equipping / parking in the grid keeps components).
///   Per the R39 dev-save policy there is NO 5 -> 6 migration
///   arrow: during development a format bump simply REFUSES older
///   files with a clear message (the pre-6 chain stays registered
///   for the release-time migration work, but currently dead-ends
///   at 5). Real cross-version compatibility is deferred to the
///   final release.
/// - v7: W479 - adds `ticks_since_rest` (the MC TIME_SINCE_REST
///   stat driving phantom insomnia). Per the W474/W476.1 precedent
///   this bump SHIPS a trivial append arrow
///   (`players_v7::player_v6_to_v7`, default 0 = well rested) so
///   the user's un-verified v6 world stays loadable; the chain
///   still dead-ends at 5 for anything older.
pub(crate) const PLAYER_FORMAT_VERSION: u32 = 7;

/// Default zstd compression level. Matches `chunks.rs` - level 3 is
/// the documented "balanced" setting from the design doc. W479:
/// `pub(super)` so the `players_v7` arrow module shares the envelope
/// helpers instead of copying them.
pub(super) const ZSTD_LEVEL: i32 = 3;

/// Compression-kind discriminant byte stored in the envelope.
pub(super) const COMP_NONE: u8 = 0;
pub(super) const COMP_ZSTD: u8 = 1;

/// Default inventory size - 9 hotbar + 27 storage = 36 slots. Mirrors
/// vanilla MC + this crate's `INVENTORY_SLOTS` so a fresh save loads
/// cleanly into the runtime even if the constant shifts later.
pub(crate) const INVENTORY_SLOTS_DEFAULT: usize = 36;

/// Default crafting grid size: 3x3 (the open inventory's craft view).
pub(crate) const CRAFT_GRID_SLOTS_DEFAULT: usize = 9;

/// Default max breath in seconds. Vanilla MC underwater air bar
/// runs ~15 s before damage starts.
pub(crate) const AIR_MAX_DEFAULT: f32 = 15.0;

/// Default max health. Vanilla MC = 20 (10 hearts).
pub(crate) const HEALTH_MAX_DEFAULT: f32 = 20.0;

/// Permission level assigned to a fresh host save. Matches the
/// `chat::perms` top tier (full admin); regular joiners drop down on
/// connect.
pub(crate) const DEFAULT_HOST_PERMISSION: u8 = 4;

/// Per-player persistent state. Survives across sessions for the
/// same player name. On a dedicated server, multiple players coexist
/// with one file each.
///
/// Block ids are stored as raw `u16` (not `BlockId`) so the wire +
/// disk schema does not pull in `infengine_voxel`. The caller maps
/// back to `BlockId` at the edge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct PlayerSave {
    /// Payload-level format version. Matches the envelope version
    /// today (both bump together); Phase B.6 may split them.
    pub format_version: u32,
    /// World-space position `[x, y, z]`. Centre-of-block on a fresh
    /// save (see `fresh_player`).
    pub pos: [f32; 3],
    /// Camera yaw (rotation around Y), radians.
    pub yaw: f32,
    /// Camera pitch (rotation around X), radians.
    pub pitch: f32,
    /// Main inventory: `INVENTORY_SLOTS` slots, each holding an
    /// `Option<ItemStack>` (W466: full stacks with components -
    /// damage travels with the save). Indexing matches the
    /// runtime: 0..HOTBAR_SLOTS = hotbar row.
    pub inventory: Vec<Option<crate::item::ItemStack>>,
    /// 3x3 crafting grid contents. Empty / partially filled grid
    /// preserves item placement across save/load. W469 (v6):
    /// stack-typed so a worn tool parked in the grid keeps its
    /// components across a reload.
    pub craft_grid: Vec<Option<crate::item::ItemStack>>,
    /// Hotbar slot the player had selected on last save.
    pub selected_slot: u8,
    /// Current breath, in seconds (clamped to `AIR_MAX_DEFAULT` on
    /// the runtime side).
    pub air: f32,
    /// Current health (clamped to `HEALTH_MAX_DEFAULT` on the
    /// runtime side).
    pub health: f32,
    /// Optional "home" point set via `/sethome`. None = unset.
    pub home_pos: Option<[f32; 3]>,
    /// Optional per-player respawn point. None = use world spawn.
    pub spawn_pos: Option<[f32; 3]>,
    /// Permission level (0..=4 from `chat::perms`).
    pub permission_level: u8,
    /// Aggregate seconds the player has been active in this save.
    pub play_time_seconds: u64,
    /// Dimension the player was in on last save. Phase F dispatches
    /// the join-time chunk stream off this field.
    pub last_dimension: String,
    /// Targeting wireframe mode (Q.2). Encoded as a `u8` for forward
    /// compatibility (unknown values round to `Auto` at load time):
    /// `0 = Auto, 1 = Build, 2 = Break`. Defaults to `Auto` on a
    /// fresh save and on migrated v1 saves.
    pub highlight_mode: u8,
    /// W8.2: current player game mode. Encoded as a `u8` mirroring
    /// `GameMode`'s discriminants: `0 = Survival, 1 = Creative`.
    /// Unknown values round to Survival on load. Defaults to
    /// Survival on a fresh save and on migrated v2 saves.
    pub game_mode: u8,
    /// W416: hunger bar (0..=20). Migrated v3 saves start full.
    pub food: f32,
    /// W416: hidden saturation buffer (0..=food). Migrated v3
    /// saves start at the MC spawn value of 5.
    pub saturation: f32,
    /// W418: running XP point total. Part of the v4 payload (v4
    /// was minted the same day and never shipped without it).
    pub xp: u32,
    /// W419: worn armor `[helmet, chest, legs, boots]`. W469 (v6):
    /// stack-typed - an equipped piece's components persist.
    pub armor: Vec<Option<crate::item::ItemStack>>,
    /// W479 (v7): MC `TIME_SINCE_REST` stat in game ticks - +1 per
    /// tick, reset to 0 by a bed night-skip. 72000+ arms the
    /// phantom insomnia roll. Migrated v6 saves start well rested.
    pub ticks_since_rest: u32,
}

/// Build a fresh `PlayerSave` for a brand-new player. `spawn` is a
/// block coord; the player position is centred on that block in X/Z
/// and stands on top of it in Y so a 1x2 hitbox doesn't intersect
/// the floor on first frame.
pub(crate) fn fresh_player(spawn: [i32; 3]) -> PlayerSave {
    PlayerSave {
        format_version: PLAYER_FORMAT_VERSION,
        pos: [
            spawn[0] as f32 + 0.5,
            spawn[1] as f32 + 0.5,
            spawn[2] as f32 + 0.5,
        ],
        yaw: 0.0,
        pitch: 0.0,
        inventory: vec![None; INVENTORY_SLOTS_DEFAULT],
        craft_grid: vec![None; CRAFT_GRID_SLOTS_DEFAULT],
        selected_slot: 0,
        air: AIR_MAX_DEFAULT,
        health: HEALTH_MAX_DEFAULT,
        home_pos: None,
        spawn_pos: None,
        permission_level: DEFAULT_HOST_PERMISSION,
        play_time_seconds: 0,
        last_dimension: "overworld".to_string(),
        highlight_mode: 0, // HighlightMode::Auto
        game_mode: 0,      // GameMode::Survival (W8.2)
        food: 20.0,        // W416: MC spawn hunger
        saturation: 5.0,
        xp: 0,
        armor: vec![None; 4],
        ticks_since_rest: 0, // W479: fresh players are well rested
    }
}

/// Canonical path of a player file:
/// `<world_dir>/players/<sanitized_name>.bin`.
///
/// The on-disk name is `sanitize_name(name)` so two players that
/// only differ by case + symbols can not stomp each other, and a
/// hostile name containing `..` / `/` cannot escape the players
/// dir.
pub(crate) fn player_path(slug: &str, name: &str) -> PathBuf {
    player_path_under(&world_dir(slug), name)
}

/// Same as `player_path` but rooted at an explicit base path - used
/// by tests + hosted-server tooling driving saves out of a non-
/// default directory.
pub(crate) fn player_path_in_dir(base: &Path, slug: &str, name: &str) -> PathBuf {
    player_path_under(&world_dir_in(base, slug), name)
}

fn player_path_under(world: &Path, name: &str) -> PathBuf {
    world
        .join("players")
        .join(format!("{}.bin", sanitize_name(name)))
}

/// Canonical players directory: `<world_dir>/players/`.
fn players_dir(slug: &str) -> PathBuf {
    world_dir(slug).join("players")
}

fn players_dir_in(base: &Path, slug: &str) -> PathBuf {
    world_dir_in(base, slug).join("players")
}

/// Filesystem-safe transform for a player display name.
///
/// Rules (deliberately stricter than `layout::slug` so a hostile
/// name can not escape the players dir and Windows + Linux agree on
/// the resulting file name):
/// - ASCII letters are lowercased.
/// - `[a-z0-9_-]` pass through as-is.
/// - Every other character (including path separators, `.`, spaces,
///   non-ASCII) collapses to `_`.
/// - Empty input becomes `"_unknown_"` so we always produce a
///   non-empty file stem.
///
/// `layout::slug` uses `-` as its collapse char + strips trailing
/// dashes; the player path needs literal `_` preserved (common in
/// MC-style names like `Notch_2`) so the rules diverge.
pub(crate) fn sanitize_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() || lower == '_' || lower == '-' {
            out.push(lower);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        return "_unknown_".to_string();
    }
    out
}

/// Encode + write a player payload to its canonical slot. Creates
/// the `players/` directory if missing.
pub(crate) fn write_player(slug: &str, name: &str, save: &PlayerSave) -> io::Result<()> {
    write_player_to(&player_path(slug, name), save)
}

/// Test-friendly write variant rooted at an explicit base path.
pub(crate) fn write_player_in_dir(
    base: &Path,
    slug: &str,
    name: &str,
    save: &PlayerSave,
) -> io::Result<()> {
    write_player_to(&player_path_in_dir(base, slug, name), save)
}

/// Read + decode a player file. Returns `Ok(None)` if the file does
/// not exist - the join handler treats a missing player file as
/// "first time on this server, mint a fresh state" and must not
/// surface that as an error.
pub(crate) fn read_player(slug: &str, name: &str) -> io::Result<Option<PlayerSave>> {
    read_player_from(&player_path(slug, name), migrate::default_registry())
}

/// Test-friendly read variant rooted at an explicit base path.
pub(crate) fn read_player_in_dir(
    base: &Path,
    slug: &str,
    name: &str,
) -> io::Result<Option<PlayerSave>> {
    read_player_from(
        &player_path_in_dir(base, slug, name),
        migrate::default_registry(),
    )
}

/// Same as `read_player_in_dir` but lets the caller inject a custom
/// migration registry - the canonical hook for migration unit tests.
pub(crate) fn read_player_in_dir_with_registry(
    base: &Path,
    slug: &str,
    name: &str,
    registry: &MigrationRegistry,
) -> io::Result<Option<PlayerSave>> {
    read_player_from(&player_path_in_dir(base, slug, name), registry)
}

/// List every persisted player in a slot. Walks
/// `<world_dir>/players/`, returns sanitized stems (file name minus
/// the `.bin` suffix), sorted alphabetically. Missing dir is not an
/// error - returns an empty vec.
pub(crate) fn list_players(slug: &str) -> io::Result<Vec<String>> {
    list_players_from(&players_dir(slug))
}

/// Test-friendly list variant rooted at an explicit base path.
pub(crate) fn list_players_in_dir(base: &Path, slug: &str) -> io::Result<Vec<String>> {
    list_players_from(&players_dir_in(base, slug))
}

fn list_players_from(dir: &Path) -> io::Result<Vec<String>> {
    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err),
    };
    let mut out: Vec<String> = Vec::new();
    for entry in read.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        // Only count `.bin` files - skip any `.tmp` left over from a
        // crashed write, plus stray notes the user might drop here.
        if path.extension().and_then(|e| e.to_str()) != Some("bin") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        out.push(stem.to_string());
    }
    out.sort();
    Ok(out)
}

fn write_player_to(path: &Path, save: &PlayerSave) -> io::Result<()> {
    let cfg = bincode::config::standard();
    let raw = bincode::serde::encode_to_vec(save, cfg)
        .map_err(|err| invalid(format!("player payload encode failed: {err}")))?;
    let uncompressed_len = raw.len() as u64;

    // Same "pick smaller" policy as `chunks.rs`: a populated
    // inventory compresses well, a fresh empty save is so small zstd
    // overhead exceeds it.
    let (kind, body): (u8, Vec<u8>) = match zstd::encode_all(&raw[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw),
    };

    let mut bytes: Vec<u8> = Vec::with_capacity(body.len() + 17);
    bytes.extend_from_slice(PLAYER_MAGIC);
    bytes.extend_from_slice(&PLAYER_FORMAT_VERSION.to_le_bytes());
    bytes.push(kind);
    bytes.extend_from_slice(&uncompressed_len.to_le_bytes());
    bytes.extend_from_slice(&body);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    atomic_save(path, &bytes)
}

fn read_player_from(path: &Path, registry: &MigrationRegistry) -> io::Result<Option<PlayerSave>> {
    let migrated_at_path = std::cell::Cell::new(false);
    let migrated_bytes_holder: std::cell::RefCell<Option<Vec<u8>>> = std::cell::RefCell::new(None);
    let parse = |bytes: &[u8]| -> io::Result<PlayerSave> {
        let (payload, did_migrate, new_bytes) = decode_player_bytes(bytes, path, registry)?;
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
fn decode_player_bytes(
    bytes: &[u8],
    path: &Path,
    registry: &MigrationRegistry,
) -> io::Result<(PlayerSave, bool, Vec<u8>)> {
    if bytes.len() < 17 {
        return Err(invalid("player file truncated header"));
    }
    if &bytes[..4] != PLAYER_MAGIC {
        return Err(invalid("player file magic mismatch"));
    }
    let disk_version = migrate::peek_envelope_version(bytes)?;
    let did_migrate = disk_version != PLAYER_FORMAT_VERSION;
    let migrated = migrate::migrate_to(
        registry,
        SaveKind::Player,
        PLAYER_FORMAT_VERSION,
        path,
        disk_version,
        bytes,
    )?;
    let post = migrated.as_slice();
    if post.len() < 17 {
        return Err(invalid("player file truncated header (post-migration)"));
    }
    if &post[..4] != PLAYER_MAGIC {
        return Err(invalid("player file magic mismatch (post-migration)"));
    }
    let version = u32::from_le_bytes([post[4], post[5], post[6], post[7]]);
    if version != PLAYER_FORMAT_VERSION {
        return Err(invalid(format!(
            "player format_version {version} not supported (expected {PLAYER_FORMAT_VERSION})"
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
                return Err(invalid("uncompressed player body length mismatch"));
            }
            body.to_vec()
        }
        COMP_ZSTD => decode_zstd(body, uncompressed_len)?,
        other => {
            return Err(invalid(format!("unknown player compression kind {other}")));
        }
    };

    let cfg = bincode::config::standard();
    let (save, _read) = bincode::serde::decode_from_slice::<PlayerSave, _>(&raw, cfg)
        .map_err(|err| invalid(format!("player payload decode failed: {err}")))?;
    Ok((save, did_migrate, migrated))
}

/// Decompress `body` via zstd into a buffer of exactly
/// `expected_len` bytes. Mirrors the chunks.rs helper - bounds the
/// output so a maliciously inflated header can't trigger an
/// unbounded allocation.
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

/// V1-shaped subset of `PlayerSave` - exactly the fields that
/// existed before the Q.2 schema bump. Used solely by the v1 -> v2
/// migration step: decode v1 bytes into this, copy fields into a
/// `PlayerSaveV2` with `highlight_mode = 0`, re-encode under v2.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct PlayerSaveV1 {
    format_version: u32,
    pos: [f32; 3],
    yaw: f32,
    pitch: f32,
    inventory: Vec<Option<(u16, u8)>>,
    craft_grid: Vec<Option<(u16, u8)>>,
    selected_slot: u8,
    air: f32,
    health: f32,
    home_pos: Option<[f32; 3]>,
    spawn_pos: Option<[f32; 3]>,
    permission_level: u8,
    play_time_seconds: u64,
    last_dimension: String,
}

/// V2-shaped subset of `PlayerSave` - the Q.2 shape (adds
/// `highlight_mode`) but without W8.2's `game_mode`. Used as the
/// intermediate payload for the v1 -> v2 arrow (output) and the
/// v2 -> v3 arrow (input).
#[derive(Debug, Clone, Deserialize, Serialize)]
struct PlayerSaveV2 {
    format_version: u32,
    pos: [f32; 3],
    yaw: f32,
    pitch: f32,
    inventory: Vec<Option<(u16, u8)>>,
    craft_grid: Vec<Option<(u16, u8)>>,
    selected_slot: u8,
    air: f32,
    health: f32,
    home_pos: Option<[f32; 3]>,
    spawn_pos: Option<[f32; 3]>,
    permission_level: u8,
    play_time_seconds: u64,
    last_dimension: String,
    highlight_mode: u8,
}

/// W416: FROZEN v3 payload shape - v2 + `game_mode`, WITHOUT
/// W416's food/saturation. The v2 -> v3 arrow encodes THIS (a live
/// `PlayerSave` there would smuggle future fields into a v3
/// envelope - the W387 containers lesson); the v3 -> v4 arrow
/// decodes it.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct PlayerSaveV3 {
    format_version: u32,
    pos: [f32; 3],
    yaw: f32,
    pitch: f32,
    inventory: Vec<Option<(u16, u8)>>,
    craft_grid: Vec<Option<(u16, u8)>>,
    selected_slot: u8,
    air: f32,
    health: f32,
    home_pos: Option<[f32; 3]>,
    spawn_pos: Option<[f32; 3]>,
    permission_level: u8,
    play_time_seconds: u64,
    last_dimension: String,
    highlight_mode: u8,
    game_mode: u8,
}

/// W466: FROZEN v4 payload shape - tuple-typed inventory, pre-
/// ItemStack. The v3 -> v4 arrow encodes THIS (the live struct is
/// v5 now - encoding it would have silently produced v5-shaped
/// bytes labelled v4, the W459 entities lesson); the v4 -> v5
/// arrow decodes it.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct PlayerSaveV4 {
    format_version: u32,
    pos: [f32; 3],
    yaw: f32,
    pitch: f32,
    inventory: Vec<Option<(u16, u8)>>,
    craft_grid: Vec<Option<(u16, u8)>>,
    selected_slot: u8,
    air: f32,
    health: f32,
    home_pos: Option<[f32; 3]>,
    spawn_pos: Option<[f32; 3]>,
    permission_level: u8,
    play_time_seconds: u64,
    last_dimension: String,
    highlight_mode: u8,
    game_mode: u8,
    food: f32,
    saturation: f32,
    xp: u32,
    armor: Vec<Option<(u16, u8)>>,
}

/// W469: FROZEN v5 payload shape - `ItemStack` inventory but still
/// tuple-typed craft grid + armor. The v4 -> v5 arrow encodes THIS
/// (the live struct is v6 now); nothing decodes it in production -
/// per the R39 dev-save policy there is no 5 -> 6 arrow, so a v5
/// file refuses with the clean unsupported-version message. The
/// shape is frozen anyway so the existing arrow keeps emitting
/// byte-identical v5 output for the release-time migration chain.
#[derive(Debug, Clone, Deserialize, Serialize)]
struct PlayerSaveV5 {
    format_version: u32,
    pos: [f32; 3],
    yaw: f32,
    pitch: f32,
    inventory: Vec<Option<crate::item::ItemStack>>,
    craft_grid: Vec<Option<(u16, u8)>>,
    selected_slot: u8,
    air: f32,
    health: f32,
    home_pos: Option<[f32; 3]>,
    spawn_pos: Option<[f32; 3]>,
    permission_level: u8,
    play_time_seconds: u64,
    last_dimension: String,
    highlight_mode: u8,
    game_mode: u8,
    food: f32,
    saturation: f32,
    xp: u32,
    armor: Vec<Option<(u16, u8)>>,
}

/// V1 -> V2 envelope migration. Decodes the v1 payload (no
/// `highlight_mode` field), re-encodes it as a v2 payload with
/// `highlight_mode = HighlightMode::Auto`. Matches the
/// containers v1 -> v2 step in shape: decompress, serde-decode the
/// old struct, re-encode the new struct, re-envelope at v2.
pub(crate) fn player_v1_to_v2(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() < 17 {
        return Err(invalid("player v1 envelope truncated"));
    }
    if &bytes[..4] != PLAYER_MAGIC {
        return Err(invalid("player v1 envelope magic mismatch"));
    }
    let on_disk_v = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if on_disk_v != 1 {
        return Err(invalid(format!(
            "player_v1_to_v2 expected v1, got v{on_disk_v}"
        )));
    }
    let kind = bytes[8];
    let uncompressed_len = u64::from_le_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        bytes[16],
    ]) as usize;
    let body = &bytes[17..];

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

    let cfg = bincode::config::standard();
    let (v1, _read) = bincode::serde::decode_from_slice::<PlayerSaveV1, _>(&raw_v1, cfg)
        .map_err(|err| invalid(format!("v1 payload decode failed: {err}")))?;

    let v2 = PlayerSaveV2 {
        // Pin to literal target so a future v3 doesn't accidentally
        // hijack the v1 -> v2 arrow.
        format_version: 2,
        pos: v1.pos,
        yaw: v1.yaw,
        pitch: v1.pitch,
        inventory: v1.inventory,
        craft_grid: v1.craft_grid,
        selected_slot: v1.selected_slot,
        air: v1.air,
        health: v1.health,
        home_pos: v1.home_pos,
        spawn_pos: v1.spawn_pos,
        permission_level: v1.permission_level,
        play_time_seconds: v1.play_time_seconds,
        last_dimension: v1.last_dimension,
        highlight_mode: 0, // HighlightMode::Auto
    };
    let raw_v2 = bincode::serde::encode_to_vec(&v2, cfg)
        .map_err(|err| invalid(format!("v2 payload encode failed: {err}")))?;
    let uncompressed_len_v2 = raw_v2.len() as u64;

    let (kind_v2, body_v2): (u8, Vec<u8>) = match zstd::encode_all(&raw_v2[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw_v2.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw_v2),
    };
    let mut out: Vec<u8> = Vec::with_capacity(body_v2.len() + 17);
    out.extend_from_slice(PLAYER_MAGIC);
    out.extend_from_slice(&2u32.to_le_bytes());
    out.push(kind_v2);
    out.extend_from_slice(&uncompressed_len_v2.to_le_bytes());
    out.extend_from_slice(&body_v2);
    Ok(out)
}

/// W8.2: V2 -> V3 envelope migration. Decodes the v2 payload
/// (no `game_mode` field), re-encodes it as a v3 payload with
/// `game_mode = GameMode::Survival`. Mirrors the v1 -> v2 step.
pub(crate) fn player_v2_to_v3(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() < 17 {
        return Err(invalid("player v2 envelope truncated"));
    }
    if &bytes[..4] != PLAYER_MAGIC {
        return Err(invalid("player v2 envelope magic mismatch"));
    }
    let on_disk_v = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if on_disk_v != 2 {
        return Err(invalid(format!(
            "player_v2_to_v3 expected v2, got v{on_disk_v}"
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
    let (v2, _read) = bincode::serde::decode_from_slice::<PlayerSaveV2, _>(&raw_v2, cfg)
        .map_err(|err| invalid(format!("v2 payload decode failed: {err}")))?;

    // W416: encode the FROZEN v3 shape, not the live PlayerSave -
    // the live struct now carries v4 fields that must not leak
    // into a v3 envelope.
    let v3 = PlayerSaveV3 {
        format_version: 3,
        pos: v2.pos,
        yaw: v2.yaw,
        pitch: v2.pitch,
        inventory: v2.inventory,
        craft_grid: v2.craft_grid,
        selected_slot: v2.selected_slot,
        air: v2.air,
        health: v2.health,
        home_pos: v2.home_pos,
        spawn_pos: v2.spawn_pos,
        permission_level: v2.permission_level,
        play_time_seconds: v2.play_time_seconds,
        last_dimension: v2.last_dimension,
        highlight_mode: v2.highlight_mode,
        game_mode: 0, // GameMode::Survival
    };
    let raw_v3 = bincode::serde::encode_to_vec(&v3, cfg)
        .map_err(|err| invalid(format!("v3 payload encode failed: {err}")))?;
    let uncompressed_len_v3 = raw_v3.len() as u64;

    let (kind_v3, body_v3): (u8, Vec<u8>) = match zstd::encode_all(&raw_v3[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw_v3.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw_v3),
    };
    let mut out: Vec<u8> = Vec::with_capacity(body_v3.len() + 17);
    out.extend_from_slice(PLAYER_MAGIC);
    out.extend_from_slice(&3u32.to_le_bytes());
    out.push(kind_v3);
    out.extend_from_slice(&uncompressed_len_v3.to_le_bytes());
    out.extend_from_slice(&body_v3);
    Ok(out)
}

/// W416: V3 -> V4 envelope migration. Decodes the FROZEN v3
/// payload (no hunger fields), re-encodes it as a v4 payload with
/// the MC spawn hunger (food 20, saturation 5). Mirrors the
/// v2 -> v3 step.
pub(crate) fn player_v3_to_v4(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() < 17 {
        return Err(invalid("player v3 envelope truncated"));
    }
    if &bytes[..4] != PLAYER_MAGIC {
        return Err(invalid("player v3 envelope magic mismatch"));
    }
    let on_disk_v = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if on_disk_v != 3 {
        return Err(invalid(format!(
            "player_v3_to_v4 expected v3, got v{on_disk_v}"
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
    let (v3, _read) = bincode::serde::decode_from_slice::<PlayerSaveV3, _>(&raw_v3, cfg)
        .map_err(|err| invalid(format!("v3 payload decode failed: {err}")))?;

    // W466: emit the FROZEN v4 shape (the live struct is v5 now).
    let v4 = PlayerSaveV4 {
        format_version: 4,
        pos: v3.pos,
        yaw: v3.yaw,
        pitch: v3.pitch,
        inventory: v3.inventory,
        craft_grid: v3.craft_grid,
        selected_slot: v3.selected_slot,
        air: v3.air,
        health: v3.health,
        home_pos: v3.home_pos,
        spawn_pos: v3.spawn_pos,
        permission_level: v3.permission_level,
        play_time_seconds: v3.play_time_seconds,
        last_dimension: v3.last_dimension,
        highlight_mode: v3.highlight_mode,
        game_mode: v3.game_mode,
        food: 20.0,
        saturation: 5.0,
        xp: 0,
        armor: vec![None; 4],
    };
    let raw_v4 = bincode::serde::encode_to_vec(&v4, cfg)
        .map_err(|err| invalid(format!("v4 payload encode failed: {err}")))?;
    let uncompressed_len_v4 = raw_v4.len() as u64;

    let (kind_v4, body_v4): (u8, Vec<u8>) = match zstd::encode_all(&raw_v4[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw_v4.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw_v4),
    };
    let mut out: Vec<u8> = Vec::with_capacity(body_v4.len() + 17);
    out.extend_from_slice(PLAYER_MAGIC);
    out.extend_from_slice(&4u32.to_le_bytes());
    out.push(kind_v4);
    out.extend_from_slice(&uncompressed_len_v4.to_le_bytes());
    out.extend_from_slice(&body_v4);
    Ok(out)
}

/// W466: V4 -> V5 envelope migration. Decodes the FROZEN v4
/// payload (tuple inventory), re-encodes it as a v5 payload whose
/// inventory slots are `ItemStack`s lifted with empty components.
/// The old per-slot durability map was runtime-only (never
/// persisted), so there is nothing to fold into `damage` here -
/// lifted tools load fresh, exactly like every pre-W466 reload
/// did. Mirrors the v3 -> v4 step.
///
/// W469/R39: the chain currently dead-ends at v5 (the live format
/// is v6 with no 5 -> 6 arrow), so this step never fires in
/// production - `migrate_to` refuses any pre-6 file up front. It
/// stays registered + tested for the release-time migration work.
pub(crate) fn player_v4_to_v5(bytes: &[u8]) -> io::Result<Vec<u8>> {
    if bytes.len() < 17 {
        return Err(invalid("player v4 envelope truncated"));
    }
    if &bytes[..4] != PLAYER_MAGIC {
        return Err(invalid("player v4 envelope magic mismatch"));
    }
    let on_disk_v = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if on_disk_v != 4 {
        return Err(invalid(format!(
            "player_v4_to_v5 expected v4, got v{on_disk_v}"
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
    let (v4, _read) = bincode::serde::decode_from_slice::<PlayerSaveV4, _>(&raw_v4, cfg)
        .map_err(|err| invalid(format!("v4 payload decode failed: {err}")))?;

    // W469: emit the FROZEN v5 shape (the live PlayerSave is v6
    // with ItemStack craft grid + armor).
    let v5 = PlayerSaveV5 {
        format_version: 5,
        pos: v4.pos,
        yaw: v4.yaw,
        pitch: v4.pitch,
        inventory: v4
            .inventory
            .into_iter()
            .map(|s| s.map(crate::item::ItemStack::from_tuple))
            .collect(),
        craft_grid: v4.craft_grid,
        selected_slot: v4.selected_slot,
        air: v4.air,
        health: v4.health,
        home_pos: v4.home_pos,
        spawn_pos: v4.spawn_pos,
        permission_level: v4.permission_level,
        play_time_seconds: v4.play_time_seconds,
        last_dimension: v4.last_dimension,
        highlight_mode: v4.highlight_mode,
        game_mode: v4.game_mode,
        food: v4.food,
        saturation: v4.saturation,
        xp: v4.xp,
        armor: v4.armor,
    };
    let raw_v5 = bincode::serde::encode_to_vec(&v5, cfg)
        .map_err(|err| invalid(format!("v5 payload encode failed: {err}")))?;
    let uncompressed_len_v5 = raw_v5.len() as u64;

    let (kind_v5, body_v5): (u8, Vec<u8>) = match zstd::encode_all(&raw_v5[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw_v5.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw_v5),
    };
    let mut out: Vec<u8> = Vec::with_capacity(body_v5.len() + 17);
    out.extend_from_slice(PLAYER_MAGIC);
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
    fn fresh_player_defaults_match_documented_constants() {
        let p = fresh_player([10, 64, -5]);
        assert_eq!(p.format_version, PLAYER_FORMAT_VERSION);
        assert_eq!(p.pos, [10.5, 64.5, -4.5]);
        assert_eq!(p.yaw, 0.0);
        assert_eq!(p.pitch, 0.0);
        assert_eq!(p.inventory.len(), INVENTORY_SLOTS_DEFAULT);
        assert!(p.inventory.iter().all(|s| s.is_none()));
        assert_eq!(p.craft_grid.len(), CRAFT_GRID_SLOTS_DEFAULT);
        assert_eq!(p.selected_slot, 0);
        assert_eq!(p.air, AIR_MAX_DEFAULT);
        assert_eq!(p.health, HEALTH_MAX_DEFAULT);
        assert!(p.home_pos.is_none());
        assert!(p.spawn_pos.is_none());
        assert_eq!(p.permission_level, DEFAULT_HOST_PERMISSION);
        assert_eq!(p.play_time_seconds, 0);
        assert_eq!(p.last_dimension, "overworld");
        assert_eq!(p.highlight_mode, 0);
        assert_eq!(p.game_mode, 0);
    }

    #[test]
    fn sanitize_name_basic_lower_and_replace() {
        assert_eq!(sanitize_name("Alice"), "alice");
        assert_eq!(sanitize_name("notch_2"), "notch_2");
        assert_eq!(sanitize_name("Cool-Player"), "cool-player");
        assert_eq!(sanitize_name("hello world"), "hello_world");
        assert_eq!(sanitize_name("a/b\\c"), "a_b_c");
        assert_eq!(sanitize_name(".."), "__");
        assert_eq!(sanitize_name(""), "_unknown_");
    }

    /// Encode a `PlayerSaveV1`-shaped struct under a v1 envelope so
    /// the v1 -> v2 migration test can run on bytes the real
    /// pre-Q.2 writer would have produced.
    fn encode_v1_envelope(v1: &PlayerSaveV1) -> Vec<u8> {
        let cfg = bincode::config::standard();
        let raw = bincode::serde::encode_to_vec(v1, cfg).unwrap();
        let uncompressed_len = raw.len() as u64;
        let mut bytes: Vec<u8> = Vec::with_capacity(raw.len() + 17);
        bytes.extend_from_slice(PLAYER_MAGIC);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(COMP_NONE);
        bytes.extend_from_slice(&uncompressed_len.to_le_bytes());
        bytes.extend_from_slice(&raw);
        bytes
    }

    #[test]
    fn player_v1_to_v2_appends_highlight_mode_auto() {
        // Build a representative v1 payload then drive the
        // migration. The result must decode as v2 with
        // `highlight_mode = 0` (= Auto).
        let v1 = PlayerSaveV1 {
            format_version: 1,
            pos: [10.5, 64.0, -5.5],
            yaw: 1.25,
            pitch: -0.5,
            inventory: vec![None; INVENTORY_SLOTS_DEFAULT],
            craft_grid: vec![None; CRAFT_GRID_SLOTS_DEFAULT],
            selected_slot: 3,
            air: 12.5,
            health: 17.5,
            home_pos: Some([0.0, 64.0, 0.0]),
            spawn_pos: None,
            permission_level: 4,
            play_time_seconds: 42,
            last_dimension: "overworld".to_string(),
        };
        let v1_bytes = encode_v1_envelope(&v1);
        let v2_bytes = player_v1_to_v2(&v1_bytes).expect("migration ok");

        // Envelope version byte must be v2.
        let on_disk_v =
            u32::from_le_bytes([v2_bytes[4], v2_bytes[5], v2_bytes[6], v2_bytes[7]]);
        assert_eq!(on_disk_v, 2);

        // Decode the v2 envelope all the way down and confirm the
        // highlight_mode rounded to Auto + every other field
        // round-tripped.
        let kind = v2_bytes[8];
        let uncompressed_len = u64::from_le_bytes([
            v2_bytes[9], v2_bytes[10], v2_bytes[11], v2_bytes[12], v2_bytes[13],
            v2_bytes[14], v2_bytes[15], v2_bytes[16],
        ]) as usize;
        let body = &v2_bytes[17..];
        let raw = match kind {
            COMP_NONE => body.to_vec(),
            COMP_ZSTD => decode_zstd(body, uncompressed_len).unwrap(),
            other => panic!("unexpected compression kind {other}"),
        };
        let cfg = bincode::config::standard();
        // Decode as the PlayerSaveV2 intermediate (W8.2 added a
        // trailing game_mode byte to the live `PlayerSave` shape,
        // so the v1 -> v2 arrow's output no longer matches v3).
        let (v2, _read) =
            bincode::serde::decode_from_slice::<PlayerSaveV2, _>(&raw, cfg).unwrap();
        assert_eq!(v2.format_version, 2);
        assert_eq!(v2.pos, v1.pos);
        assert_eq!(v2.yaw, v1.yaw);
        assert_eq!(v2.pitch, v1.pitch);
        assert_eq!(v2.selected_slot, v1.selected_slot);
        assert_eq!(v2.air, v1.air);
        assert_eq!(v2.health, v1.health);
        assert_eq!(v2.home_pos, v1.home_pos);
        assert_eq!(v2.spawn_pos, v1.spawn_pos);
        assert_eq!(v2.permission_level, v1.permission_level);
        assert_eq!(v2.play_time_seconds, v1.play_time_seconds);
        assert_eq!(v2.last_dimension, v1.last_dimension);
        assert_eq!(v2.highlight_mode, 0);
    }

    #[test]
    fn player_v1_to_v2_rejects_non_v1_input() {
        // Driving the v1 -> v2 step on a non-v1 envelope must
        // surface an error rather than corrupting the bytes.
        let mut bytes = vec![0u8; 17];
        bytes[..4].copy_from_slice(PLAYER_MAGIC);
        // Claim v2 on disk.
        bytes[4..8].copy_from_slice(&2u32.to_le_bytes());
        let err = player_v1_to_v2(&bytes).expect_err("must reject v2");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn player_v1_refused_cleanly_under_dev_policy() {
        // W469/R39: the registered chain dead-ends at v5 while the
        // live format is v6 - a v1 file through the PRODUCTION
        // reader must refuse with the clean unsupported-version
        // message (naming family + found/expected versions), NOT
        // half-migrate or panic. The pre-6 arrows stay covered by
        // their per-step unit tests above.
        let v1 = PlayerSaveV1 {
            format_version: 1,
            pos: [0.5, 64.0, 0.5],
            yaw: 0.0,
            pitch: 0.0,
            inventory: vec![None; INVENTORY_SLOTS_DEFAULT],
            craft_grid: vec![None; CRAFT_GRID_SLOTS_DEFAULT],
            selected_slot: 0,
            air: AIR_MAX_DEFAULT,
            health: HEALTH_MAX_DEFAULT,
            home_pos: None,
            spawn_pos: None,
            permission_level: DEFAULT_HOST_PERMISSION,
            play_time_seconds: 0,
            last_dimension: "overworld".to_string(),
        };
        let v1_bytes = encode_v1_envelope(&v1);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = player_path_in_dir(dir.path(), "q2", "tester");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &v1_bytes).unwrap();
        let err = read_player_in_dir(dir.path(), "q2", "tester")
            .expect_err("v1 must refuse under the R39 dev policy");
        let msg = err.to_string();
        assert!(msg.contains("player"), "names the family: {msg}");
        assert!(msg.contains('1'), "names the found version: {msg}");
        assert!(
            msg.contains(&PLAYER_FORMAT_VERSION.to_string()),
            "names the expected version: {msg}"
        );
    }

    /// Encode a `PlayerSaveV2`-shaped struct under a v2 envelope so
    /// the v2 -> v3 migration test can run on the bytes the real
    /// pre-W8.2 writer would have produced.
    fn encode_v2_envelope(v2: &PlayerSaveV2) -> Vec<u8> {
        let cfg = bincode::config::standard();
        let raw = bincode::serde::encode_to_vec(v2, cfg).unwrap();
        let uncompressed_len = raw.len() as u64;
        let mut bytes: Vec<u8> = Vec::with_capacity(raw.len() + 17);
        bytes.extend_from_slice(PLAYER_MAGIC);
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.push(COMP_NONE);
        bytes.extend_from_slice(&uncompressed_len.to_le_bytes());
        bytes.extend_from_slice(&raw);
        bytes
    }

    fn sample_v2_payload() -> PlayerSaveV2 {
        PlayerSaveV2 {
            format_version: 2,
            pos: [4.5, 70.0, -3.5],
            yaw: 0.5,
            pitch: -0.2,
            inventory: vec![None; INVENTORY_SLOTS_DEFAULT],
            craft_grid: vec![None; CRAFT_GRID_SLOTS_DEFAULT],
            selected_slot: 2,
            air: AIR_MAX_DEFAULT,
            health: 18.0,
            home_pos: None,
            spawn_pos: Some([0.0, 64.0, 0.0]),
            permission_level: DEFAULT_HOST_PERMISSION,
            play_time_seconds: 99,
            last_dimension: "overworld".to_string(),
            highlight_mode: 1,
        }
    }

    #[test]
    fn player_v2_to_v3_appends_game_mode_survival() {
        // W8.2: v2 payloads migrate to v3 with `game_mode = 0`
        // (Survival). Every other field round-trips byte-identical.
        let v2 = sample_v2_payload();
        let v2_bytes = encode_v2_envelope(&v2);
        let v3_bytes = player_v2_to_v3(&v2_bytes).expect("migration ok");
        let on_disk_v =
            u32::from_le_bytes([v3_bytes[4], v3_bytes[5], v3_bytes[6], v3_bytes[7]]);
        assert_eq!(on_disk_v, 3);

        let kind = v3_bytes[8];
        let uncompressed_len = u64::from_le_bytes([
            v3_bytes[9], v3_bytes[10], v3_bytes[11], v3_bytes[12], v3_bytes[13],
            v3_bytes[14], v3_bytes[15], v3_bytes[16],
        ]) as usize;
        let body = &v3_bytes[17..];
        let raw = match kind {
            COMP_NONE => body.to_vec(),
            COMP_ZSTD => decode_zstd(body, uncompressed_len).unwrap(),
            other => panic!("unexpected compression kind {other}"),
        };
        let cfg = bincode::config::standard();
        // W416: the arrow emits the FROZEN v3 shape (no hunger
        // fields) - decoding the live PlayerSave here would demand
        // v4 fields a v3 envelope must not contain.
        let (v3, _read) =
            bincode::serde::decode_from_slice::<PlayerSaveV3, _>(&raw, cfg).unwrap();
        assert_eq!(v3.format_version, 3);
        assert_eq!(v3.highlight_mode, v2.highlight_mode);
        assert_eq!(v3.last_dimension, v2.last_dimension);
        assert_eq!(v3.game_mode, 0);
    }

    /// W416: v3 -> v4 appends the hunger pair at the MC spawn
    /// state; a full v1 -> v4 chain reaches the same defaults.
    #[test]
    fn player_v3_to_v4_appends_spawn_hunger() {
        let v3 = PlayerSaveV3 {
            format_version: 3,
            pos: [1.5, 70.0, -2.5],
            yaw: 0.3,
            pitch: -0.1,
            inventory: vec![None; INVENTORY_SLOTS_DEFAULT],
            craft_grid: vec![None; CRAFT_GRID_SLOTS_DEFAULT],
            selected_slot: 2,
            air: 9.0,
            health: 13.5,
            home_pos: None,
            spawn_pos: Some([4.0, 65.0, 4.0]),
            permission_level: 4,
            play_time_seconds: 7,
            last_dimension: "nether".to_string(),
            highlight_mode: 1,
            game_mode: 1,
        };
        let cfg = bincode::config::standard();
        let raw = bincode::serde::encode_to_vec(&v3, cfg).unwrap();
        let mut bytes: Vec<u8> = Vec::with_capacity(raw.len() + 17);
        bytes.extend_from_slice(PLAYER_MAGIC);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.push(COMP_NONE);
        bytes.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&raw);

        let v4_bytes = player_v3_to_v4(&bytes).expect("migration ok");
        assert_eq!(
            u32::from_le_bytes([
                v4_bytes[4], v4_bytes[5], v4_bytes[6], v4_bytes[7]
            ]),
            4
        );
        let kind = v4_bytes[8];
        let uncompressed_len = u64::from_le_bytes([
            v4_bytes[9], v4_bytes[10], v4_bytes[11], v4_bytes[12],
            v4_bytes[13], v4_bytes[14], v4_bytes[15], v4_bytes[16],
        ]) as usize;
        let body = &v4_bytes[17..];
        let raw4 = match kind {
            COMP_NONE => body.to_vec(),
            COMP_ZSTD => decode_zstd(body, uncompressed_len).unwrap(),
            other => panic!("unexpected compression kind {other}"),
        };
        // W466: the arrow emits the FROZEN v4 shape now (the live
        // PlayerSave is v5 with ItemStack slots).
        let (v4, _read) =
            bincode::serde::decode_from_slice::<PlayerSaveV4, _>(&raw4, cfg)
                .unwrap();
        assert_eq!(v4.format_version, 4);
        assert_eq!(v4.health, 13.5);
        assert_eq!(v4.game_mode, 1);
        assert_eq!(v4.food, 20.0);
        assert_eq!(v4.saturation, 5.0);
        assert_eq!(v4.xp, 0);
        assert_eq!(v4.armor, vec![None; 4]);
    }

    /// W466: a hand-built v4 envelope lifts through the 4 -> 5
    /// arrow with every inventory tuple promoted to an `ItemStack`
    /// with empty components; all other v4 fields carry through.
    #[test]
    fn player_v4_to_v5_lifts_item_stacks() {
        let cfg = bincode::config::standard();
        let mut inventory: Vec<Option<(u16, u8)>> =
            vec![None; INVENTORY_SLOTS_DEFAULT];
        inventory[0] = Some((1, 64));
        inventory[3] = Some((48, 1)); // flint+steel (was map-tracked)
        let v4 = PlayerSaveV4 {
            format_version: 4,
            pos: [2.5, 71.0, -3.5],
            yaw: 0.7,
            pitch: 0.2,
            inventory,
            craft_grid: vec![None; CRAFT_GRID_SLOTS_DEFAULT],
            selected_slot: 3,
            air: 8.0,
            health: 12.0,
            home_pos: None,
            spawn_pos: Some([1.0, 66.0, 1.0]),
            permission_level: 4,
            play_time_seconds: 11,
            last_dimension: "the_end".to_string(),
            highlight_mode: 2,
            game_mode: 1,
            food: 14.0,
            saturation: 2.5,
            xp: 77,
            armor: vec![Some((312, 1)), None, None, None],
        };
        let raw = bincode::serde::encode_to_vec(&v4, cfg).unwrap();
        let mut bytes: Vec<u8> = Vec::with_capacity(raw.len() + 17);
        bytes.extend_from_slice(PLAYER_MAGIC);
        bytes.extend_from_slice(&4u32.to_le_bytes());
        bytes.push(COMP_NONE);
        bytes.extend_from_slice(&(raw.len() as u64).to_le_bytes());
        bytes.extend_from_slice(&raw);

        let out = player_v4_to_v5(&bytes).expect("arrow lifts");
        assert_eq!(
            u32::from_le_bytes([out[4], out[5], out[6], out[7]]),
            5
        );
        let kind = out[8];
        let len = u64::from_le_bytes(out[9..17].try_into().unwrap()) as usize;
        let body: Vec<u8> = match kind {
            COMP_NONE => out[17..].to_vec(),
            _ => decode_zstd(&out[17..], len).unwrap(),
        };
        // W469: the arrow emits the FROZEN v5 shape (the live
        // PlayerSave is v6 with stack craft grid + armor).
        let (v5, _read) =
            bincode::serde::decode_from_slice::<PlayerSaveV5, _>(&body, cfg)
                .unwrap();
        assert_eq!(v5.format_version, 5);
        assert_eq!(
            v5.inventory[0],
            Some(crate::item::ItemStack::new(1, 64))
        );
        assert_eq!(
            v5.inventory[3],
            Some(crate::item::ItemStack::new(48, 1))
        );
        assert!(
            v5.inventory[3]
                .as_ref()
                .unwrap()
                .components
                .is_empty(),
            "lifted stacks start with empty components"
        );
        assert!(v5.inventory[1].is_none());
        // v4 fields carry through.
        assert_eq!(v5.health, 12.0);
        assert_eq!(v5.game_mode, 1);
        assert_eq!(v5.food, 14.0);
        assert_eq!(v5.saturation, 2.5);
        assert_eq!(v5.xp, 77);
        assert_eq!(v5.armor[0], Some((312, 1)));
        assert_eq!(v5.last_dimension, "the_end");
    }

    #[test]
    fn player_v4_to_v5_rejects_non_v4_input() {
        // Driving the v4 -> v5 step on a non-v4 envelope must
        // surface an error rather than corrupting bytes.
        let mut bytes = vec![0u8; 17];
        bytes[..4].copy_from_slice(PLAYER_MAGIC);
        bytes[4..8].copy_from_slice(&5u32.to_le_bytes());
        let err = player_v4_to_v5(&bytes).expect_err("must reject v5");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn player_v2_to_v3_rejects_non_v2_input() {
        // Driving the v2 -> v3 step on a non-v2 envelope must
        // surface an error rather than corrupting bytes.
        let mut bytes = vec![0u8; 17];
        bytes[..4].copy_from_slice(PLAYER_MAGIC);
        bytes[4..8].copy_from_slice(&3u32.to_le_bytes());
        let err = player_v2_to_v3(&bytes).expect_err("must reject v3");
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn player_v2_refused_cleanly_under_dev_policy() {
        // W469/R39: same as the v1 case - the production reader
        // refuses every pre-6 version with the clean message.
        let v2 = sample_v2_payload();
        let v2_bytes = encode_v2_envelope(&v2);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = player_path_in_dir(dir.path(), "w8", "alice");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &v2_bytes).unwrap();
        let err = read_player_in_dir(dir.path(), "w8", "alice")
            .expect_err("v2 must refuse under the R39 dev policy");
        let msg = err.to_string();
        assert!(msg.contains("player"), "names the family: {msg}");
        assert!(
            msg.contains(&PLAYER_FORMAT_VERSION.to_string()),
            "names the expected version: {msg}"
        );
    }

    #[test]
    fn fresh_player_defaults_game_mode_to_survival() {
        // W8.2: minted players default to Survival.
        let p = fresh_player([0, 64, 0]);
        assert_eq!(p.game_mode, 0);
    }

    #[test]
    fn player_save_round_trips_game_mode_via_envelope() {
        // W8.2: a fresh v3 PlayerSave with a non-default
        // game_mode survives write -> read unchanged.
        let mut save = fresh_player([1, 64, 1]);
        save.game_mode = 1; // Creative
        let dir = tempfile::tempdir().expect("tempdir");
        write_player_in_dir(dir.path(), "w8", "bob", &save).unwrap();
        let loaded = read_player_in_dir(dir.path(), "w8", "bob")
            .expect("read ok")
            .expect("save present");
        assert_eq!(loaded.game_mode, 1);
        assert_eq!(loaded.format_version, PLAYER_FORMAT_VERSION);
    }
}
