//! Wave 12.7 - `.jar` cache discovery + reuse infrastructure.
//!
//! The user's MC install may ship the version as either:
//! - a pre-extracted directory tree
//!   (`<minecraft>/versions/26.1.2/assets/...`), OR
//! - a single `<version>.jar` archive
//!   (`<minecraft>/versions/26.1.2/26.1.2.jar`).
//!
//! The Wave 9 path discovery handles the first case. This module
//! adds the cache-side support for the second:
//! - `jar_cache_dir(data_dir, version)` returns the engine's
//!   `<data_dir>/cache/mc_assets/<version>/` cache root.
//! - `cache_status(jar_path, cache_dir)` reports whether the
//!   cache exists, is fresh (mtime matches the .jar), or needs
//!   re-extraction.
//! - `mark_cache_extracted(jar_path, cache_dir)` writes a tiny
//!   sentinel `.extracted_from_mtime` file that subsequent boots
//!   compare against the source `.jar` mtime.
//!
//! The actual `.jar` -> `cache_dir` extraction lives in a future
//! commit gated on the `zip` crate addition; the discovery side
//! here is the prerequisite. Without the cache layer the engine
//! would have to re-walk + re-decompress the archive on every
//! boot.
//!
//! Cite W9.7 (skipped) deferred note: "`.jar` auto-extraction -
//! user with only `26.1.2.jar` should auto-extract on first load."

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Sentinel filename written to the cache dir on a successful
/// extraction. Holds the source `.jar` mtime as a u64 ns
/// timestamp; subsequent boots compare against the live mtime
/// to decide whether to re-extract.
pub(crate) const CACHE_MTIME_SENTINEL: &str = ".extracted_from_mtime";

/// What the cache layer thinks about a given `(jar, cache_dir)`
/// pair. The engine startup walks this enum to decide whether to
/// invoke the actual extractor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CacheStatus {
    /// Cache dir exists and its sentinel mtime matches the .jar.
    /// Skip extraction; the cache is ready to serve.
    Fresh,
    /// Cache dir exists but the sentinel is missing or out-of-
    /// date. Re-extract.
    Stale,
    /// Cache dir is absent. First-time extraction needed.
    Missing,
}

/// Compute the engine's cache directory for a given MC version.
/// Layout: `<data_dir>/cache/mc_assets/<version>/`.
pub(crate) fn jar_cache_dir(data_dir: &Path, version: &str) -> PathBuf {
    data_dir.join("cache").join("mc_assets").join(version)
}

/// Read the source `.jar` mtime as a `u64` of nanoseconds-since-
/// epoch. Returns `None` when the file doesn't exist or the
/// platform mtime API is unavailable.
fn jar_mtime_ns(jar_path: &Path) -> Option<u64> {
    let meta = fs::metadata(jar_path).ok()?;
    let mtime = meta.modified().ok()?;
    let dur = mtime.duration_since(SystemTime::UNIX_EPOCH).ok()?;
    let ns = dur.as_secs() as u128 * 1_000_000_000 + dur.subsec_nanos() as u128;
    Some(ns as u64)
}

/// Read the cache sentinel's stored mtime, if any.
fn cache_sentinel_ns(cache_dir: &Path) -> Option<u64> {
    let p = cache_dir.join(CACHE_MTIME_SENTINEL);
    let s = fs::read_to_string(&p).ok()?;
    s.trim().parse::<u64>().ok()
}

/// Decide whether the cache at `cache_dir` is ready to serve.
pub(crate) fn cache_status(jar_path: &Path, cache_dir: &Path) -> CacheStatus {
    if !cache_dir.is_dir() {
        return CacheStatus::Missing;
    }
    let Some(jar_ns) = jar_mtime_ns(jar_path) else {
        // No .jar file - treat as missing so the engine falls back
        // to the placeholder path. Caller logs the absence.
        return CacheStatus::Missing;
    };
    let Some(sentinel_ns) = cache_sentinel_ns(cache_dir) else {
        return CacheStatus::Stale;
    };
    if sentinel_ns == jar_ns {
        CacheStatus::Fresh
    } else {
        CacheStatus::Stale
    }
}

/// Write the cache sentinel after a successful extraction.
pub(crate) fn mark_cache_extracted(
    jar_path: &Path,
    cache_dir: &Path,
) -> io::Result<()> {
    fs::create_dir_all(cache_dir)?;
    let mtime = jar_mtime_ns(jar_path)
        .ok_or_else(|| io::Error::other("could not read jar mtime"))?;
    let p = cache_dir.join(CACHE_MTIME_SENTINEL);
    fs::write(&p, mtime.to_string())?;
    Ok(())
}

/// Filter applied to entries inside the .jar: we only keep what the
/// resource-pack loader actually consumes. Reduces extract time +
/// disk usage from ~60 MB (full jar) to a few MB (just assets +
/// `pack.mcmeta`).
fn keep_entry(name: &str) -> bool {
    name.starts_with("assets/")
        || name == "pack.mcmeta"
        || name == "pack.png"
}

/// Extract the relevant subtree of `jar_path` into `cache_dir`.
/// Idempotent + atomic-ish: if the cache is `Fresh`, returns without
/// doing work; on success writes the mtime sentinel last so a
/// partial extract leaves the cache marked `Stale` for retry.
///
/// Strips the `assets/` etc. paths into `cache_dir` verbatim - the
/// resource-pack loader's well-known layout (`textures/block/...`)
/// lives under `cache_dir/assets/minecraft/textures/...` once the
/// extract finishes.
pub(crate) fn ensure_extracted(
    jar_path: &Path,
    cache_dir: &Path,
) -> io::Result<()> {
    if cache_status(jar_path, cache_dir) == CacheStatus::Fresh {
        return Ok(());
    }
    // Wipe a stale cache so we don't mix old + new files. Cheap: at
    // worst we re-extract a few MB.
    if cache_dir.is_dir() {
        fs::remove_dir_all(cache_dir)?;
    }
    fs::create_dir_all(cache_dir)?;

    let f = fs::File::open(jar_path)?;
    let mut archive = zip::ZipArchive::new(f)
        .map_err(|e| io::Error::other(format!("zip open: {e}")))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| io::Error::other(format!("zip entry: {e}")))?;
        // ZipFile::name returns the entry path. Use enclosed_name
        // to refuse `..` traversal attempts before we touch the FS.
        let Some(rel) = entry.enclosed_name() else {
            continue;
        };
        let Some(rel_str) = rel.to_str() else {
            continue;
        };
        if !keep_entry(rel_str) {
            continue;
        }
        let out = cache_dir.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut sink = fs::File::create(&out)?;
        io::copy(&mut entry, &mut sink)?;
    }
    // Sentinel last so a partially-extracted cache stays Stale.
    mark_cache_extracted(jar_path, cache_dir)?;
    Ok(())
}

/// Detect whether the user's MC install has only the `.jar` (no
/// pre-extracted dir). The W9 path discovery already covers the
/// extracted case; this helper tells the caller "extract before
/// proceeding".
///
/// Returns `Some(jar_path)` when only the `.jar` is present;
/// `None` when:
/// - the version dir is missing entirely (no MC install).
/// - the version dir contains an `assets/` subtree
///   (pre-extracted, use that directly).
pub(crate) fn needs_extraction(versions_dir: &Path, version: &str) -> Option<PathBuf> {
    let version_dir = versions_dir.join(version);
    if !version_dir.is_dir() {
        return None;
    }
    // If the version dir holds an extracted `assets/` tree the
    // caller doesn't need to extract.
    if version_dir.join("assets").is_dir() {
        return None;
    }
    let jar = version_dir.join(format!("{version}.jar"));
    if jar.is_file() {
        Some(jar)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_root(label: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "infengine_jar_cache_{}_{}_{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::create_dir_all(&p).expect("temp mkdir");
        p
    }

    #[test]
    fn jar_cache_dir_layout() {
        let root = PathBuf::from("/data");
        let p = jar_cache_dir(&root, "26.1.2");
        assert!(p.ends_with("cache/mc_assets/26.1.2") || p.ends_with("cache\\mc_assets\\26.1.2"));
    }

    #[test]
    fn jar_extract_creates_cache_dir() {
        // mark_cache_extracted writes the sentinel AND ensures the
        // cache dir exists. Simulates a successful extraction.
        let root = temp_root("extract");
        let jar = root.join("test.jar");
        fs::write(&jar, b"fake").expect("write jar");
        let cache = root.join("cache");
        // Pre-condition: no cache dir.
        assert!(!cache.is_dir());
        mark_cache_extracted(&jar, &cache).expect("mark");
        // Post-condition: cache dir + sentinel exist.
        assert!(cache.is_dir());
        assert!(cache.join(CACHE_MTIME_SENTINEL).is_file());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn jar_extract_reuses_cache_on_second_load() {
        // 1. Create jar + mark extraction.
        // 2. cache_status should report Fresh.
        let root = temp_root("reuse");
        let jar = root.join("test.jar");
        fs::write(&jar, b"fake jar").expect("write jar");
        let cache = root.join("cache");
        mark_cache_extracted(&jar, &cache).expect("mark");
        assert_eq!(cache_status(&jar, &cache), CacheStatus::Fresh);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn cache_missing_when_dir_absent() {
        let root = temp_root("missing");
        let jar = root.join("test.jar");
        fs::write(&jar, b"x").expect("write");
        let cache = root.join("absent");
        assert_eq!(cache_status(&jar, &cache), CacheStatus::Missing);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn cache_stale_when_sentinel_missing() {
        // Cache dir exists but no sentinel file -> Stale.
        let root = temp_root("stale_no_sent");
        let jar = root.join("test.jar");
        fs::write(&jar, b"x").expect("write");
        let cache = root.join("cache");
        fs::create_dir_all(&cache).expect("mkdir");
        assert_eq!(cache_status(&jar, &cache), CacheStatus::Stale);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn cache_stale_when_sentinel_mtime_mismatches() {
        let root = temp_root("stale_mtime");
        let jar = root.join("test.jar");
        fs::write(&jar, b"x").expect("write");
        let cache = root.join("cache");
        fs::create_dir_all(&cache).expect("mkdir");
        // Write a sentinel with a deliberately-wrong mtime.
        fs::write(
            cache.join(CACHE_MTIME_SENTINEL),
            "99999999999999999",
        )
        .expect("write sentinel");
        assert_eq!(cache_status(&jar, &cache), CacheStatus::Stale);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn extracted_cache_used_by_path_discovery() {
        // Simulate: user has 26.1.2.jar but no pre-extracted dir.
        // After we mark the cache extracted (the future extraction
        // call), the cache_dir is what the engine's pack loader
        // points its asset-root at.
        let root = temp_root("path_discover");
        let versions = root.join("versions").join("26.1.2");
        fs::create_dir_all(&versions).expect("mkdir versions");
        let jar = versions.join("26.1.2.jar");
        fs::write(&jar, b"fake jar").expect("write jar");
        let need = needs_extraction(&root.join("versions"), "26.1.2");
        assert!(
            need.is_some(),
            "should detect needs extraction when only .jar present",
        );
        // Pretend we've extracted. Now the cache is the new asset root.
        let cache = jar_cache_dir(&root, "26.1.2");
        mark_cache_extracted(&jar, &cache).expect("mark");
        assert_eq!(cache_status(&jar, &cache), CacheStatus::Fresh);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn needs_extraction_returns_none_when_assets_dir_exists() {
        let root = temp_root("has_assets");
        let versions = root.join("versions").join("26.1.2");
        fs::create_dir_all(versions.join("assets")).expect("mkdir");
        fs::write(versions.join("26.1.2.jar"), b"x").expect("write jar");
        // Pre-extracted - no extraction needed.
        assert!(needs_extraction(&root.join("versions"), "26.1.2").is_none());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn needs_extraction_returns_none_when_no_install_at_all() {
        let root = temp_root("no_install");
        // No versions dir at all.
        assert!(needs_extraction(&root.join("versions"), "26.1.2").is_none());
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn extracted_cache_dir_layout_is_versioned() {
        let root = PathBuf::from("/data");
        let a = jar_cache_dir(&root, "26.1.2");
        let b = jar_cache_dir(&root, "26.1.3");
        // Different version -> different cache dir.
        assert_ne!(a, b);
    }

    #[test]
    fn cache_sentinel_filename_is_dotfile() {
        // Sentinel must start with `.` so a directory listing
        // doesn't accidentally include it as a real asset.
        assert!(CACHE_MTIME_SENTINEL.starts_with('.'));
    }

    #[test]
    fn mark_cache_extracted_fails_when_jar_missing() {
        let root = temp_root("mark_no_jar");
        let cache = root.join("cache");
        let jar = root.join("missing.jar");
        let res = mark_cache_extracted(&jar, &cache);
        assert!(res.is_err());
        fs::remove_dir_all(&root).ok();
    }

    /// Build a tiny in-memory zip with the entries we care about
    /// (an `assets/minecraft/textures/block/stone.png` + a stray
    /// non-assets entry) so the test can exercise the filter.
    fn make_fake_jar(out: &Path) {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        let f = fs::File::create(out).expect("create jar");
        let mut zw = zip::ZipWriter::new(f);
        let opts = SimpleFileOptions::default();
        zw.start_file("assets/minecraft/textures/block/stone.png", opts)
            .unwrap();
        zw.write_all(b"PNG bytes").unwrap();
        zw.start_file("META-INF/MANIFEST.MF", opts).unwrap();
        zw.write_all(b"junk").unwrap();
        zw.start_file("pack.mcmeta", opts).unwrap();
        zw.write_all(b"{}").unwrap();
        zw.finish().unwrap();
    }

    #[test]
    fn ensure_extracted_pulls_only_assets() {
        let root = temp_root("extract_filter");
        let jar = root.join("test.jar");
        make_fake_jar(&jar);
        let cache = root.join("cache");
        ensure_extracted(&jar, &cache).expect("extract");
        assert!(
            cache
                .join("assets")
                .join("minecraft")
                .join("textures")
                .join("block")
                .join("stone.png")
                .is_file(),
            "asset PNG must extract"
        );
        assert!(
            cache.join("pack.mcmeta").is_file(),
            "pack.mcmeta must extract"
        );
        // META-INF is dropped by the keep_entry filter.
        assert!(
            !cache.join("META-INF").exists(),
            "META-INF should be skipped"
        );
        assert_eq!(cache_status(&jar, &cache), CacheStatus::Fresh);
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ensure_extracted_is_idempotent_when_fresh() {
        let root = temp_root("extract_idempotent");
        let jar = root.join("test.jar");
        make_fake_jar(&jar);
        let cache = root.join("cache");
        ensure_extracted(&jar, &cache).expect("first");
        let stone = cache
            .join("assets/minecraft/textures/block/stone.png")
            .clone();
        // Drop the file under the cache to prove the second call
        // does NOT re-extract (cache is Fresh -> early return).
        fs::remove_file(&stone).ok();
        ensure_extracted(&jar, &cache).expect("second");
        assert!(
            !stone.is_file(),
            "Fresh cache must skip the extract pass entirely"
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ensure_extracted_refreshes_stale_cache() {
        let root = temp_root("extract_stale");
        let jar = root.join("test.jar");
        make_fake_jar(&jar);
        let cache = root.join("cache");
        // Plant a bogus pre-existing file + a wrong sentinel so the
        // status is Stale.
        fs::create_dir_all(&cache).unwrap();
        fs::write(cache.join("bogus.txt"), b"old").unwrap();
        fs::write(cache.join(CACHE_MTIME_SENTINEL), "12345").unwrap();
        assert_eq!(cache_status(&jar, &cache), CacheStatus::Stale);
        ensure_extracted(&jar, &cache).expect("extract");
        // Old file gone; new ones present; status now Fresh.
        assert!(!cache.join("bogus.txt").exists());
        assert!(
            cache
                .join("assets/minecraft/textures/block/stone.png")
                .is_file()
        );
        assert_eq!(cache_status(&jar, &cache), CacheStatus::Fresh);
        fs::remove_dir_all(&root).ok();
    }
}
