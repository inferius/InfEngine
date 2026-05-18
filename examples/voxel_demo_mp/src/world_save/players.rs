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

/// Magic bytes at the head of every player file: `"INFP"` (InfEngine
/// player). Distinguishes the format from chunks (`INFC`) + the
/// legacy `INFV` state blob.
pub(crate) const PLAYER_MAGIC: &[u8; 4] = b"INFP";

/// Envelope + payload format version. Bumped when the on-disk shape
/// changes in a breaking way. Phase B.6 wires a migration registry.
pub(crate) const PLAYER_FORMAT_VERSION: u32 = 1;

/// Default zstd compression level. Matches `chunks.rs` - level 3 is
/// the documented "balanced" setting from the design doc.
const ZSTD_LEVEL: i32 = 3;

/// Compression-kind discriminant byte stored in the envelope.
const COMP_NONE: u8 = 0;
const COMP_ZSTD: u8 = 1;

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
    /// `Option<(block_id_u16, count_u8)>`. Indexing matches the
    /// runtime: 0..HOTBAR_SLOTS = hotbar row.
    pub inventory: Vec<Option<(u16, u8)>>,
    /// 3x3 crafting grid contents. Empty / partially filled grid
    /// preserves item placement across save/load.
    pub craft_grid: Vec<Option<(u16, u8)>>,
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
    read_player_from(&player_path(slug, name))
}

/// Test-friendly read variant rooted at an explicit base path.
pub(crate) fn read_player_in_dir(
    base: &Path,
    slug: &str,
    name: &str,
) -> io::Result<Option<PlayerSave>> {
    read_player_from(&player_path_in_dir(base, slug, name))
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

fn read_player_from(path: &Path) -> io::Result<Option<PlayerSave>> {
    read_with_recovery(path, decode_player_bytes)
}

/// Parse the on-disk envelope + bincode payload. Free fn so both the
/// primary read and the `.bak` recovery path inside
/// `atomic::read_with_recovery` run the exact same validation.
fn decode_player_bytes(bytes: &[u8]) -> io::Result<PlayerSave> {
    if bytes.len() < 17 {
        return Err(invalid("player file truncated header"));
    }
    if &bytes[..4] != PLAYER_MAGIC {
        return Err(invalid("player file magic mismatch"));
    }
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if version != PLAYER_FORMAT_VERSION {
        return Err(invalid(format!(
            "player format_version {version} not supported (expected {PLAYER_FORMAT_VERSION})"
        )));
    }
    let kind = bytes[8];
    let uncompressed_len = u64::from_le_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        bytes[16],
    ]) as usize;
    let body = &bytes[17..];

    let raw: Vec<u8> = match kind {
        COMP_NONE => {
            if body.len() != uncompressed_len {
                return Err(invalid("uncompressed player body length mismatch"));
            }
            body.to_vec()
        }
        COMP_ZSTD => decode_zstd(body, uncompressed_len)?,
        other => {
            return Err(invalid(format!(
                "unknown player compression kind {other}"
            )));
        }
    };

    let cfg = bincode::config::standard();
    let (save, _read) = bincode::serde::decode_from_slice::<PlayerSave, _>(&raw, cfg)
        .map_err(|err| invalid(format!("player payload decode failed: {err}")))?;
    Ok(save)
}

/// Decompress `body` via zstd into a buffer of exactly
/// `expected_len` bytes. Mirrors the chunks.rs helper - bounds the
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
            return Err(invalid(
                "zstd decode exceeded uncompressed_payload_len",
            ));
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
}
