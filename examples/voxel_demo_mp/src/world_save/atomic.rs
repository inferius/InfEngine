//! Atomic file writes + 1-deep rolling backup + crash recovery.
//!
//! Every persisted file in `world_save` goes through `atomic_save`:
//!
//! 1. Write the new payload to a sibling `<path>.tmp`.
//! 2. If `<path>` exists, rename it to `<path>.bak` (overwriting any
//!    previous backup). This is the rolling 1-deep history slot the
//!    design doc calls for.
//! 3. Rename `<path>.tmp` onto `<path>`.
//!
//! A crash between steps 1 and 3 can only ever corrupt the `.tmp`
//! sibling - `<path>` and `<path>.bak` always hold the two most
//! recent good versions.
//!
//! `read_with_recovery` is the read-side companion. It tries
//! `<path>` first; on parse failure it falls back to `<path>.bak`,
//! restores the good backup onto `<path>`, and stashes the corrupt
//! original at `<path>.broken.YYYYMMDD-HHMMSS-mmm` so a human can
//! poke at it offline.
//!
//! See `docs/design/save_architecture_final.md` ("Atomicity +
//! corruption recovery") for the canonical contract.

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use tracing::{info, warn};

/// Atomically write `payload` to `path` with a 1-deep rolling
/// backup at `<path>.bak`.
///
/// Sequence (each step a separate `fs` syscall - no shell glue):
///
/// 1. `fs::create_dir_all(path.parent())` - first save into a fresh
///    slot must not require the caller to pre-create the layout.
/// 2. `fs::write(tmp, payload)` with `fsync` - the payload is durable
///    on disk before we touch the live file.
/// 3. If `path` exists: `fs::rename(path, bak)`. Overwrites any prior
///    `.bak` (Windows needs an explicit `remove_file` first).
/// 4. `fs::rename(tmp, path)`.
///
/// On Linux step 4 is atomic-over-existing; on Windows step 3 is
/// what guarantees the destination is empty by the time step 4 hits.
pub(crate) fn atomic_save(path: &Path, payload: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = tmp_path(path);
    let bak = bak_path(path);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(payload)?;
        f.sync_all()?;
    }
    // Rotate the existing live file into the backup slot. Drop any
    // stale `.bak` first so Windows `rename` doesn't refuse the
    // overwrite.
    if path.exists() {
        if bak.exists() {
            let _ = std::fs::remove_file(&bak);
        }
        std::fs::rename(path, &bak)?;
    }
    std::fs::rename(&tmp, path)
}

/// Read `path` and parse via the caller-supplied closure, falling
/// back to `<path>.bak` on parse failure.
///
/// Return values:
///
/// - `Ok(None)` - neither `path` nor `<path>.bak` exists. The caller
///   treats this as "no saved state yet" (fresh world, first join,
///   chunk regenerated from worldgen, ...).
/// - `Ok(Some(value))` - either `path` parsed cleanly, OR `path` was
///   corrupt and `<path>.bak` parsed cleanly. In the second case the
///   corrupt original is moved aside to
///   `<path>.broken.YYYYMMDD-HHMMSS-mmm` and the backup is renamed
///   back onto `<path>` so the recovered version becomes the new live
///   file (the next `atomic_save` will then write the recovered
///   version into a fresh `.bak`).
/// - `Err(_)` - both files exist but neither parses. The function
///   leaves both `.broken` snapshots on disk so a human can pull
///   them out of the slot for forensics.
///
/// I/O errors other than "file missing" propagate immediately.
pub(crate) fn read_with_recovery<R>(
    path: &Path,
    parse: impl Fn(&[u8]) -> io::Result<R>,
) -> io::Result<Option<R>> {
    let bak = bak_path(path);
    let primary_exists = path.exists();
    let bak_exists = bak.exists();
    if !primary_exists && !bak_exists {
        return Ok(None);
    }

    // Try `path` first.
    if primary_exists {
        match read_and_parse(path, &parse) {
            Ok(value) => return Ok(Some(value)),
            Err(err) => {
                warn!(
                    path = %path.display(),
                    error = %err,
                    "atomic: primary parse failed; attempting .bak recovery"
                );
                // Fall through to the .bak branch below.
                if !bak_exists {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!(
                            "atomic: {} corrupt + no .bak available: {err}",
                            path.display()
                        ),
                    ));
                }
            }
        }
    }

    // `.bak` is our last resort. If it parses, restore it onto
    // `path` + stash the corrupt original (if any) at `.broken.<ts>`.
    match read_and_parse(&bak, &parse) {
        Ok(value) => {
            if primary_exists {
                // Move the corrupt primary aside before we overwrite.
                let broken = broken_path(path);
                match std::fs::rename(path, &broken) {
                    Ok(_) => {
                        info!(
                            path = %path.display(),
                            broken = %broken.display(),
                            "atomic: stashed corrupt primary at .broken.<ts>"
                        );
                    }
                    Err(err) => {
                        warn!(
                            path = %path.display(),
                            error = %err,
                            "atomic: failed to stash corrupt primary; dropping it"
                        );
                        let _ = std::fs::remove_file(path);
                    }
                }
            }
            // Promote the good `.bak` back to the live slot. The next
            // `atomic_save` will mint a fresh `.bak` from the now-live
            // recovered version.
            std::fs::rename(&bak, path)?;
            info!(
                path = %path.display(),
                "atomic: recovered from .bak"
            );
            Ok(Some(value))
        }
        Err(err_bak) => {
            // Both corrupt. Snapshot whichever side still exists so a
            // human can poke at them later.
            if primary_exists {
                let broken = broken_path(path);
                let _ = std::fs::rename(path, &broken);
            }
            let broken_bak = broken_path(&bak);
            let _ = std::fs::rename(&bak, &broken_bak);
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "atomic: both {} and .bak corrupt: {err_bak}",
                    path.display()
                ),
            ))
        }
    }
}

/// Read `path` into memory and hand the bytes to `parse`. Wraps the
/// fs error with the file path so the caller sees which file failed.
fn read_and_parse<R>(
    path: &Path,
    parse: impl Fn(&[u8]) -> io::Result<R>,
) -> io::Result<R> {
    let bytes = std::fs::read(path)?;
    parse(&bytes)
}

/// `<path>.tmp` - the staging file used by `atomic_save`. Replaces
/// the existing file's extension so `chunks/0_0_0.chunk.bin` becomes
/// `chunks/0_0_0.chunk.tmp`. Matches the legacy `meta::atomic_write`
/// behaviour so a partially-deployed build doesn't strand `.tmp`
/// files the new code can't recognise.
fn tmp_path(path: &Path) -> PathBuf {
    path.with_extension("tmp")
}

/// `<path>.bak` - the 1-deep rolling backup. Appends `.bak` to the
/// FULL path (extension included) so the original extension stays
/// inside the backup name. Examples:
///
/// - `meta.json` -> `meta.json.bak`
/// - `chunks/0_0_0.chunk.bin` -> `chunks/0_0_0.chunk.bin.bak`
/// - `noext` -> `noext.bak`
///
/// This is deliberate: a `with_extension("bak")` on `meta.json` would
/// emit `meta.bak`, which collides with the `meta` (no-extension)
/// slot if such a file ever existed.
fn bak_path(path: &Path) -> PathBuf {
    let mut s: OsString = path.as_os_str().to_owned();
    s.push(".bak");
    PathBuf::from(s)
}

/// `<path>.broken.YYYYMMDD-HHMMSS-mmm` - the forensic snapshot
/// landing pad. Millisecond precision so two recoveries inside the
/// same second don't collide. UTC because save files travel
/// between machines and "local" timestamps surprise people.
fn broken_path(path: &Path) -> PathBuf {
    let now = chrono::Utc::now();
    let stamp = now.format("%Y%m%d-%H%M%S-%3f").to_string();
    let mut s: OsString = path.as_os_str().to_owned();
    s.push(".broken.");
    s.push(stamp);
    PathBuf::from(s)
}
