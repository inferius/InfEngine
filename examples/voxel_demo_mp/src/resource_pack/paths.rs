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
pub(crate) fn discover_assets_root() -> Option<PathBuf> {
    for candidate in candidate_paths() {
        if let Some(root) = canonicalize_assets_root(candidate)
            && root.join(CANARY).exists()
        {
            return Some(root);
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
    if let Some(appdata) = appdata_roaming() {
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
    use std::sync::Mutex;

    /// Global mutex serializing env-mutating tests. `std::env::set_var`
    /// is unsafe in multi-threaded code (other threads may be reading
    /// the same key), and cargo runs tests in parallel by default;
    /// this lock keeps env-mutating tests from racing each other.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// RAII multi-key env guard. Holds the global env lock for its
    /// lifetime + restores every previously-mutated key on drop, so
    /// tests can safely fence on parallel runs.
    struct EnvScope {
        prev: Vec<(&'static str, Option<String>)>,
        _g: std::sync::MutexGuard<'static, ()>,
    }
    impl EnvScope {
        fn new() -> Self {
            let g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
            Self {
                prev: Vec::new(),
                _g: g,
            }
        }
        fn set(&mut self, key: &'static str, val: &str) -> &mut Self {
            let prev = std::env::var(key).ok();
            // SAFETY: ENV_LOCK serializes all env mutation.
            unsafe { std::env::set_var(key, val) };
            self.prev.push((key, prev));
            self
        }
        fn unset(&mut self, key: &'static str) -> &mut Self {
            let prev = std::env::var(key).ok();
            // SAFETY: ENV_LOCK serializes all env mutation.
            unsafe { std::env::remove_var(key) };
            self.prev.push((key, prev));
            self
        }
    }
    impl Drop for EnvScope {
        fn drop(&mut self) {
            // SAFETY: the guard still holds ENV_LOCK at this point.
            unsafe {
                while let Some((key, prev)) = self.prev.pop() {
                    match prev {
                        Some(v) => std::env::set_var(key, v),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
    }

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
