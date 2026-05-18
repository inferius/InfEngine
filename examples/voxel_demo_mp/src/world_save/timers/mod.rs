//! Scheduled wall-clock timers + time-warp catch-up dispatch.
//!
//! See `docs/design/save_architecture_final.md` ("timers/scheduled.bin"
//! + "Time-warp catch-up") for the canonical contract. Wire summary:
//!
//! ```text
//! [u8;  4]   magic = b"INFT"
//! [u32]      format_version  (little-endian)
//! [u8]       compression_kind (0 = none, 1 = zstd)
//! [u64]      uncompressed_payload_len (little-endian)
//! [bytes]    payload (bincode-serialised ScheduledTimers, optionally
//!            zstd-compressed)
//! ```
//!
//! Same envelope shape as the rest of `world_save` - one decoder per
//! file family. The bundle is small (a few KB even on a populated
//! save) so the compressor mostly serves the per-timer text fields
//! like the dim string.
//!
//! ## Catch-up
//!
//! On server boot the runtime calls `catch_up(timers, now_wall_ms)`.
//! For every timer whose `due_at_wall_ms <= cutoff` (= last save +
//! capped elapsed), a per-kind catch-up fn runs the simulation
//! forward in pure code and emits a list of `CatchUpEvent`s the
//! caller applies to the live world. The cap (`MAX_OFFLINE_MS` = 7
//! days) keeps a year-old save from spending minutes simulating on
//! load.
//!
//! Per-kind rules:
//! - Furnace: one tick = 50 ms. While `fuel > 0 && input > 0`, fuel
//!   drains and cook progresses. At cook_progress >= 200 ticks (10 s)
//!   emit one `FurnaceOutput`, reset progress, consume one input.
//! - Crop: stages advance one step per `GROWTH_PERIOD_MS` (default
//!   300 000 ms = 5 min). At `max_stage` emit `CropHarvested` + drop
//!   the timer; below max keep it with the bumped stage + new due.
//! - LiquidScheduled: emit `LiquidScheduled` event + drop the timer.
//!   Caller re-hydrates the live `LiquidSchedule` from the event.
//!
//! Phase B.7 is what wires this into the live save / load path.
//! Right now the module is exercised purely by the unit tests in
//! `world_save/timers_tests.rs`.
//!
//! ## File layout
//!
//! - `mod.rs` (this file): types, on-disk envelope encode + decode,
//!   path resolution. Stays under the 500-LOC soft cap.
//! - `catch_up.rs`: the `catch_up` entry point + per-kind dispatchers.
//!   Pulled out so each side of the module stays focused.
#![allow(dead_code)]

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::atomic::{atomic_save, read_with_recovery};
use super::layout::{world_dir, world_dir_in};

pub(crate) mod catch_up;

// Re-export the catch-up entry points at the parent module level so
// existing call sites (and tests) can keep using `super::timers::*`
// without having to reach down into the sub-module path.
#[allow(unused_imports)]
pub(crate) use catch_up::{CatchUpEvent, CatchUpResult, catch_up};

/// Magic bytes at the head of every timers file: `"INFT"` (InfEngine
/// timers). Distinguishes the format from chunks (`INFC`), players
/// (`INFP`), and entities (`INFE`).
pub(crate) const TIMERS_MAGIC: &[u8; 4] = b"INFT";

/// Envelope + payload format version. Bumped when the on-disk shape
/// changes in a breaking way. Phase B.6 wires a migration registry.
pub(crate) const TIMERS_FORMAT_VERSION: u32 = 1;

/// Default zstd compression level. Matches the rest of `world_save` -
/// level 3 is the documented "balanced" setting from the design doc.
const ZSTD_LEVEL: i32 = 3;

/// Compression-kind discriminant byte stored in the envelope.
const COMP_NONE: u8 = 0;
const COMP_ZSTD: u8 = 1;

/// Catch-up cap. A save older than this gets treated as exactly this
/// many ms of offline time so a year-old save doesn't spend minutes
/// simulating on load. Default = 7 days, matches the design doc.
pub(crate) const MAX_OFFLINE_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// Game-tick length in ms. Matches the runtime tick rate; furnace
/// cook progress + fuel are measured in these ticks.
pub(crate) const TICK_MS: u64 = 50;

/// Cook ticks per furnace output. 200 ticks * 50 ms/tick = 10 s, the
/// vanilla-MC smelt time. Tweakable per-recipe later - for now the
/// catch-up uses this single fixed value because Phase C hasn't
/// landed per-recipe metadata yet.
pub(crate) const FURNACE_COOK_TICKS_PER_OUTPUT: u32 = 200;

/// Wall-ms between crop growth stage advances. Default 5 min =
/// 300 000 ms. Picked to roughly match vanilla wheat (~5 min from
/// stage 0 to fully grown) and to make the catch-up math easy: 15 min
/// elapsed = exactly 3 stages.
pub(crate) const GROWTH_PERIOD_MS: u64 = 5 * 60 * 1000;

/// All persistent scheduled timers across all dimensions.
///
/// Live file: `<world_dir>/timers/scheduled.bin`. Single file rather
/// than per-dim because the typical population is dozens-to-hundreds
/// of timers and a single sequential scan on boot is cheaper than the
/// directory walk we'd need for a per-dim split.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct ScheduledTimers {
    /// Payload-level format version. Matches the envelope version
    /// today (both bump together); Phase B.6 may split them.
    pub format_version: u32,
    /// Every active timer at save time. Order is preserved but
    /// callers must not rely on it - the catch-up may reshuffle.
    pub items: Vec<Timer>,
    /// Wall-clock ms at the previous save. The catch-up delta is
    /// `now - last_saved_wall_ms` (capped at `MAX_OFFLINE_MS`).
    pub last_saved_wall_ms: u64,
}

impl ScheduledTimers {
    /// Build a fresh empty bundle at the current format version.
    /// `now_wall_ms` seeds `last_saved_wall_ms` so a brand-new world
    /// doesn't trigger an unbounded "elapsed since 1970" catch-up on
    /// the next boot.
    pub(crate) fn empty(now_wall_ms: u64) -> Self {
        Self {
            format_version: TIMERS_FORMAT_VERSION,
            items: Vec::new(),
            last_saved_wall_ms: now_wall_ms,
        }
    }
}

/// One scheduled timer. `kind` carries the per-kind payload + drives
/// the catch-up dispatch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct Timer {
    /// World-space block position the timer is attached to. Caller
    /// resolves block <-> timer at the edge.
    pub pos: [i32; 3],
    /// Dimension name (e.g. "overworld"). Stored as `String` for the
    /// same reason `last_dimension` lives on `PlayerSave` - keeps the
    /// schema free of the runtime `Dimension` enum.
    pub dim: String,
    /// Wall-clock ms when the timer next fires. The catch-up checks
    /// this against `now` to decide whether to dispatch.
    pub due_at_wall_ms: u64,
    /// Per-kind state + dispatcher discriminator.
    pub kind: TimerKind,
}

/// Per-kind timer payload. New kinds land here via a fresh variant +
/// a matching `catch_up_*` helper; old saves keep loading because the
/// loader refuses unknown discriminators only via the envelope
/// version (variants in a serde enum are append-only-safe).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) enum TimerKind {
    /// One furnace cook step. The catch-up advances cook progress +
    /// fuel use over the elapsed window, emitting one
    /// `FurnaceOutput` per completed cook.
    Furnace {
        /// Ticks of fuel still in the burner.
        fuel_left_ticks: u32,
        /// Ticks of cook progress accumulated on the current input
        /// (0..`FURNACE_COOK_TICKS_PER_OUTPUT`).
        cook_progress_ticks: u32,
        /// Block id currently being smelted. `0` = empty slot, drops
        /// the timer once cook finishes its in-flight item.
        input_block: u16,
        /// Block id emitted on each completed cook.
        output_block: u16,
    },
    /// One crop growth step. Catch-up advances `stage` toward
    /// `max_stage`; at the cap it emits `CropHarvested` and drops
    /// the timer.
    Crop {
        /// Current growth stage (0..=`max_stage`).
        stage: u8,
        /// Final stage. At this value the crop is "ripe" and the next
        /// catch-up tick fires the harvest event.
        max_stage: u8,
    },
    /// One scheduled liquid-spread tick. The catch-up just emits the
    /// `LiquidScheduled` event - the caller re-hydrates its in-memory
    /// `LiquidSchedule` from those events.
    LiquidScheduled {
        /// Liquid id (water, lava, ...) stored as raw `u16` so the
        /// schema does not pull in the runtime's liquid registry.
        liquid_id: u16,
    },
}

/// Canonical path of the scheduled-timers file:
/// `<world_dir>/timers/scheduled.bin`.
pub(crate) fn timers_path(slug: &str) -> PathBuf {
    timers_path_under(&world_dir(slug))
}

/// Same as `timers_path` but rooted at an explicit base path. Used
/// by tests + the future hosted-server tooling that drives saves out
/// of a non-default directory.
pub(crate) fn timers_path_in_dir(base: &Path, slug: &str) -> PathBuf {
    timers_path_under(&world_dir_in(base, slug))
}

fn timers_path_under(world: &Path) -> PathBuf {
    world.join("timers").join("scheduled.bin")
}

/// Encode + write the scheduled-timers bundle to its canonical slot.
/// Creates the `timers/` directory if missing.
pub(crate) fn write_timers(slug: &str, timers: &ScheduledTimers) -> io::Result<()> {
    write_timers_to(&timers_path(slug), timers)
}

/// Test-friendly write variant rooted at an explicit base path.
pub(crate) fn write_timers_in_dir(
    base: &Path,
    slug: &str,
    timers: &ScheduledTimers,
) -> io::Result<()> {
    write_timers_to(&timers_path_in_dir(base, slug), timers)
}

/// Read + decode the scheduled-timers bundle. Returns `Ok(None)` if
/// the file does not exist - server boot treats a missing bundle as
/// "no scheduled timers yet" (fresh world, no furnaces) and must not
/// surface that as an error.
pub(crate) fn read_timers(slug: &str) -> io::Result<Option<ScheduledTimers>> {
    read_timers_from(&timers_path(slug))
}

/// Test-friendly read variant rooted at an explicit base path.
pub(crate) fn read_timers_in_dir(
    base: &Path,
    slug: &str,
) -> io::Result<Option<ScheduledTimers>> {
    read_timers_from(&timers_path_in_dir(base, slug))
}

fn write_timers_to(path: &Path, timers: &ScheduledTimers) -> io::Result<()> {
    let cfg = bincode::config::standard();
    let raw = bincode::serde::encode_to_vec(timers, cfg)
        .map_err(|err| invalid(format!("timers payload encode failed: {err}")))?;
    let uncompressed_len = raw.len() as u64;

    // Same "pick smaller" policy as the rest of `world_save`: a
    // populated bundle with many timers compresses well via zstd; a
    // tiny near-empty file is so small the zstd overhead exceeds it.
    let (kind, body): (u8, Vec<u8>) = match zstd::encode_all(&raw[..], ZSTD_LEVEL) {
        Ok(compressed) if compressed.len() < raw.len() => (COMP_ZSTD, compressed),
        _ => (COMP_NONE, raw),
    };

    let mut bytes: Vec<u8> = Vec::with_capacity(body.len() + 17);
    bytes.extend_from_slice(TIMERS_MAGIC);
    bytes.extend_from_slice(&TIMERS_FORMAT_VERSION.to_le_bytes());
    bytes.push(kind);
    bytes.extend_from_slice(&uncompressed_len.to_le_bytes());
    bytes.extend_from_slice(&body);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    atomic_save(path, &bytes)
}

fn read_timers_from(path: &Path) -> io::Result<Option<ScheduledTimers>> {
    read_with_recovery(path, decode_timers_bytes)
}

/// Parse the on-disk envelope + bincode payload. Free fn so both the
/// primary read and the `.bak` recovery path inside
/// `atomic::read_with_recovery` run the exact same validation.
fn decode_timers_bytes(bytes: &[u8]) -> io::Result<ScheduledTimers> {
    if bytes.len() < 17 {
        return Err(invalid("timers file truncated header"));
    }
    if &bytes[..4] != TIMERS_MAGIC {
        return Err(invalid("timers file magic mismatch"));
    }
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    if version != TIMERS_FORMAT_VERSION {
        return Err(invalid(format!(
            "timers format_version {version} not supported (expected {TIMERS_FORMAT_VERSION})"
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
                return Err(invalid("uncompressed timers body length mismatch"));
            }
            body.to_vec()
        }
        COMP_ZSTD => decode_zstd(body, uncompressed_len)?,
        other => {
            return Err(invalid(format!(
                "unknown timers compression kind {other}"
            )));
        }
    };

    let cfg = bincode::config::standard();
    let (timers, _read) =
        bincode::serde::decode_from_slice::<ScheduledTimers, _>(&raw, cfg)
            .map_err(|err| invalid(format!("timers payload decode failed: {err}")))?;
    Ok(timers)
}

/// Decompress `body` via zstd into a buffer of exactly `expected_len`
/// bytes. Mirrors the chunks.rs / players.rs / entities.rs helpers -
/// bounds the output so a maliciously inflated header can't trigger
/// an unbounded allocation.
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
    fn empty_constructor_uses_current_format_version() {
        let t = ScheduledTimers::empty(1_000);
        assert_eq!(t.format_version, TIMERS_FORMAT_VERSION);
        assert!(t.items.is_empty());
        assert_eq!(t.last_saved_wall_ms, 1_000);
    }

    #[test]
    fn growth_period_locks_at_five_minutes() {
        // Tweaking GROWTH_PERIOD_MS is a deliberate breaking change
        // for existing saves - test pins the value.
        assert_eq!(GROWTH_PERIOD_MS, 5 * 60 * 1000);
    }

    #[test]
    fn furnace_cook_ticks_locked() {
        // Same rationale as the growth period: bumping this is a
        // breaking change for in-flight cooks on existing saves.
        assert_eq!(FURNACE_COOK_TICKS_PER_OUTPUT, 200);
    }

    #[test]
    fn tick_ms_locked() {
        assert_eq!(TICK_MS, 50);
    }
}
