//! MC assets path discovery.
//!
//! The probe sequence (in order):
//! 1. `INFENGINE_MC_ASSETS` env var. If set, must point at either an
//!    `assets/minecraft/` dir OR a dir CONTAINING that subtree.
//! 2. `%APPDATA%/.minecraft/versions/26.1.2/26.1.2/assets/minecraft/`
//!    (standard Mojang launcher layout - the unpacked assets live
//!    next to the version's `.jar`).
//! 3. `%APPDATA%/.minecraft/versions/<dir>/<dir>/assets/minecraft/`
//!    where the version dir starts with `26.1` so a point release
//!    (`26.1.3`, `26.1.4`, ...) still works without reconfiguration.
//! 4. `%APPDATA%/.minecraft/assets/minecraft/` (an older flat layout).
//!
//! The probe stops at the first hit that ALSO contains the canary
//! file `textures/block/stone.png` - a sanity check so a half-extracted
//! pack doesn't fool the loader into thinking it's complete.

use std::path::{Path, PathBuf};

/// Canary file that every legitimate MC `assets/minecraft/` tree has
/// since vanilla 1.x. The discovery refuses to return paths that
/// can't see it.
const CANARY: &str = "textures/block/stone.png";

/// Public entry point - run the 4-tier probe and return the first
/// path that looks like a real MC assets root, or `None`.
///
/// Tier 5 (W12.7) kicks in only when the standard probe finds nothing:
/// we look for a `<version>.jar` next to the missing extracted dir
/// and, on first hit, extract its `assets/` subtree into the engine
/// cache (`<data_dir>/cache/mc_assets/<version>/`). Subsequent boots
/// reuse the cache because the cache mtime sentinel matches.
pub(crate) fn discover_assets_root() -> Option<PathBuf> {
    for candidate in candidate_paths() {
        if let Some(root) = canonicalize_assets_root(candidate)
            && root.join(CANARY).exists()
        {
            return Some(root);
        }
    }
    // Tier 5: try the .jar-extract path.
    extract_jar_if_available()
}

/// Scan the standard MC versions dir for any `<ver>.jar` whose
/// matching extracted tree is missing; extract on first hit + return
/// the resulting cache root. Skips silently when no MC install is
/// present.
fn extract_jar_if_available() -> Option<PathBuf> {
    let appdata = appdata_roaming()?;
    let versions_dir = appdata.join(".minecraft").join("versions");
    let entries = std::fs::read_dir(&versions_dir).ok()?;
    let mut versions: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().to_string();
            if n.starts_with("26.1") {
                Some(n)
            } else {
                None
            }
        })
        .collect();
    versions.sort();
    let data = crate::world_save::layout::data_dir();
    for ver in &versions {
        let Some(jar) = crate::jar_cache::needs_extraction(&versions_dir, ver) else {
            continue;
        };
        let cache = crate::jar_cache::jar_cache_dir(&data, ver);
        if let Err(err) = crate::jar_cache::ensure_extracted(&jar, &cache) {
            tracing::warn!(
                version = %ver,
                jar = %jar.display(),
                cache = %cache.display(),
                error = %err,
                "MC jar extract failed - skipping version"
            );
            continue;
        }
        let nested = cache.join("assets").join("minecraft");
        if nested.join(CANARY).exists() {
            tracing::info!(
                version = %ver,
                cache = %nested.display(),
                "MC assets extracted from .jar into engine cache"
            );
            return Some(nested);
        }
    }
    None
}

/// Normalize a user-supplied path into an `assets/minecraft/` root.
/// Accepts:
/// - a path that IS the `minecraft/` folder,
/// - a path that contains an `assets/minecraft/` subtree.
///
/// Does not require the canary file (the caller filters).
pub(crate) fn canonicalize_assets_root<P: AsRef<Path>>(p: P) -> Option<PathBuf> {
    let p = p.as_ref();
    if !p.exists() {
        return None;
    }
    // Case 1: already the `minecraft/` folder.
    if p.file_name().and_then(|n| n.to_str()) == Some("minecraft")
        && p.join("textures").is_dir()
    {
        return Some(p.to_path_buf());
    }
    // Case 2: contains `assets/minecraft/`.
    let nested = p.join("assets").join("minecraft");
    if nested.join("textures").is_dir() {
        return Some(nested);
    }
    // Case 3: the user passed the version folder directly
    // (`%APPDATA%/.minecraft/versions/26.1.2/`); try its inner
    // duplicated version dir layout.
    if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
        let inner = p.join(name).join("assets").join("minecraft");
        if inner.join("textures").is_dir() {
            return Some(inner);
        }
    }
    None
}

/// Build the ordered list of candidate paths to probe.
fn candidate_paths() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    if let Ok(env) = std::env::var("INFENGINE_MC_ASSETS")
        && !env.trim().is_empty()
    {
        out.push(PathBuf::from(env));
    }
    // W67.x: in test builds, skip the ambient `%APPDATA%/.minecraft`
    // probe so tests stay hermetic - they must not pick up a real MC
    // install on the dev box (e.g. the `F:\Data\mc_assets` junctions),
    // which would flip the placeholder-pack assertions + slow every
    // `load()` down with 1.4 GB of disk I/O. Tests drive discovery
    // through the explicit `INFENGINE_MC_ASSETS` env above (set via
    // the test EnvScope) or `load_with_override`. The real game
    // (non-test build) keeps the full probe.
    if !cfg!(test)
        && let Some(appdata) = appdata_roaming()
    {
        let mc_root = appdata.join(".minecraft");
        // Known exact version.
        out.push(
            mc_root
                .join("versions")
                .join("26.1.2")
                .join("26.1.2")
                .join("assets")
                .join("minecraft"),
        );
        // Glob over 26.1.* (best-effort; std-only).
        if let Ok(rd) = std::fs::read_dir(mc_root.join("versions")) {
            let mut versions: Vec<PathBuf> = rd
                .flatten()
                .filter_map(|e| {
                    let n = e.file_name();
                    let name = n.to_string_lossy().to_string();
                    if name.starts_with("26.1") {
                        Some(e.path())
                    } else {
                        None
                    }
                })
                .collect();
            // Stable sort so the discovery is deterministic.
            versions.sort();
            for v in versions {
                if let Some(name) = v.file_name().and_then(|n| n.to_str()) {
                    out.push(v.join(name).join("assets").join("minecraft"));
                }
            }
        }
        // Older flat layout.
        out.push(mc_root.join("assets").join("minecraft"));
    }
    out
}

/// Locate the user's `%APPDATA%/Roaming` directory. On non-Windows
/// hosts (where this loader still compiles for tests) we fall back
/// to `$HOME/.config` so unit tests can stage a fake layout.
fn appdata_roaming() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("APPDATA")
        && !p.trim().is_empty()
    {
        return Some(PathBuf::from(p));
    }
    // POSIX fallback for unit tests / CI.
    if let Ok(p) = std::env::var("HOME")
        && !p.trim().is_empty()
    {
        return Some(PathBuf::from(p).join(".config"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    // W67.x: the env guard moved to the shared `test_env` module so
    // the paths discovery tests + the resource_pack integration tests
    // serialise env mutation through one lock (no cross-module race).
    use super::super::test_env::EnvScope;

    #[test]
    fn canonicalize_rejects_nonexistent_path() {
        let p = std::path::PathBuf::from("Z:/definitely/does/not/exist/anywhere");
        assert_eq!(canonicalize_assets_root(&p), None);
    }

    #[test]
    fn canonicalize_accepts_direct_minecraft_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path().join("minecraft");
        std::fs::create_dir_all(mc.join("textures").join("block")).unwrap();
        let canon = canonicalize_assets_root(&mc);
        assert_eq!(canon, Some(mc));
    }

    #[test]
    fn canonicalize_accepts_parent_with_assets_subdir() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("assets").join("minecraft").join("textures").join("block");
        std::fs::create_dir_all(&nested).unwrap();
        let canon = canonicalize_assets_root(tmp.path());
        assert_eq!(canon, Some(tmp.path().join("assets").join("minecraft")));
    }

    #[test]
    fn discover_uses_env_var_when_set() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path().join("minecraft");
        std::fs::create_dir_all(mc.join("textures").join("block")).unwrap();
        // Stage the canary so the probe accepts the path.
        std::fs::write(mc.join("textures").join("block").join("stone.png"), b"x").unwrap();
        let mut env = EnvScope::new();
        env.set("INFENGINE_MC_ASSETS", mc.to_str().unwrap());
        let got = discover_assets_root();
        assert_eq!(got, Some(mc));
    }

    #[test]
    fn discover_returns_none_when_env_path_lacks_canary() {
        let tmp = tempfile::tempdir().unwrap();
        let mc = tmp.path().join("minecraft");
        std::fs::create_dir_all(mc.join("textures").join("block")).unwrap();
        // No canary file - the probe must reject.
        let empty = tempfile::tempdir().unwrap();
        let mut env = EnvScope::new();
        env.set("INFENGINE_MC_ASSETS", mc.to_str().unwrap())
            .set("APPDATA", empty.path().to_str().unwrap())
            .set("HOME", empty.path().to_str().unwrap());
        let got = discover_assets_root();
        assert_eq!(got, None);
    }

    #[test]
    fn discover_returns_none_when_no_assets_on_disk() {
        // Point all three probe inputs at empty temp dirs so we
        // know there is no MC install anywhere the probe can see.
        let tmp = tempfile::tempdir().unwrap();
        let mut env = EnvScope::new();
        env.unset("INFENGINE_MC_ASSETS")
            .set("APPDATA", tmp.path().to_str().unwrap())
            .set("HOME", tmp.path().to_str().unwrap());
        let got = discover_assets_root();
        assert_eq!(got, None);
    }
}
