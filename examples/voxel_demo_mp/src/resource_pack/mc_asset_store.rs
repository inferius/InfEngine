//! W57 - MC content-addressable asset store loader.
//!
//! MC's launcher stores its assets under `assets/` keyed by SHA-1
//! hashes instead of logical paths:
//!
//! ```text
//! assets/
//!   indexes/
//!     <id>.json     <- {"objects": {"<logical>": {"hash": ..., "size": ...}}}
//!   objects/
//!     <hh>/<full_hash>   <- raw file bytes (no extension)
//! ```
//!
//! The launcher unpacks this when running the game; vanilla
//! sounds (.ogg) + langs + some textures only exist here, NOT in
//! the `assets/minecraft/` decompiled-pack tree the other
//! resource_pack code already walks. Adding this loader lets the
//! engine pick up sounds straight from a launcher install (or a
//! cached copy at `J:/Temp/MC-Sounds/assets`).
//!
//! Discovery order (mirrors `paths::discover_assets_root` but
//! looking for `indexes/` + `objects/` instead of
//! `textures/block/`):
//! 1. `INFENGINE_MC_ASSET_STORE` env var.
//! 2. `%APPDATA%/.minecraft/assets/`.
//! 3. None.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Parsed index entry from `indexes/<id>.json`. Logical path
/// (`minecraft/sounds/mob/zombie/say1.ogg`) -> SHA-1 hex hash.
#[derive(Debug, Clone)]
pub(crate) struct AssetStore {
    pub(crate) index: HashMap<String, String>,
    pub(crate) objects_dir: PathBuf,
}

impl AssetStore {
    /// Build an `AssetStore` from `assets_dir`. Returns `None` when
    /// the dir is missing the required `indexes/` + `objects/`
    /// sibling subtrees, or when no index JSON parses.
    pub(crate) fn load(assets_dir: &Path) -> Option<Self> {
        let indexes_dir = assets_dir.join("indexes");
        let objects_dir = assets_dir.join("objects");
        if !indexes_dir.is_dir() || !objects_dir.is_dir() {
            return None;
        }
        // Pick the highest-numbered index file (latest MC version).
        // Index filenames are bare integers (`17.json`, `24.json`,
        // `30.json`, ...) by Mojang convention.
        let mut ids: Vec<(u32, PathBuf)> = std::fs::read_dir(&indexes_dir)
            .ok()?
            .flatten()
            .filter_map(|e| {
                let p = e.path();
                let stem = p.file_stem()?.to_str()?;
                stem.parse::<u32>().ok().map(|n| (n, p))
            })
            .collect();
        ids.sort_by_key(|(n, _)| *n);
        let (_, path) = ids.last()?;
        let text = std::fs::read_to_string(path).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        let objects = v.get("objects")?.as_object()?;
        let mut index: HashMap<String, String> = HashMap::new();
        for (logical, entry) in objects {
            if let Some(hash) = entry.get("hash").and_then(|h| h.as_str()) {
                index.insert(logical.clone(), hash.to_string());
            }
        }
        tracing::info!(
            entries = index.len(),
            path = %path.display(),
            "MC asset-store index loaded",
        );
        Some(Self {
            index,
            objects_dir,
        })
    }

    /// Resolve a logical asset path (`minecraft/sounds/...`) to the
    /// matching object file on disk. Returns `None` when the path
    /// isn't in the index OR the hashed object file is missing.
    pub(crate) fn resolve(&self, logical: &str) -> Option<PathBuf> {
        let hash = self.index.get(logical)?;
        if hash.len() < 2 {
            return None;
        }
        let sub = &hash[..2];
        let path = self.objects_dir.join(sub).join(hash);
        if path.is_file() { Some(path) } else { None }
    }
}

/// W57 entry point: discover an MC asset store on disk. Returns
/// `None` when no candidate path exists.
pub(crate) fn discover_asset_store() -> Option<AssetStore> {
    for cand in candidate_paths() {
        if let Some(store) = AssetStore::load(&cand) {
            return Some(store);
        }
    }
    None
}

fn candidate_paths() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    if let Ok(env) = std::env::var("INFENGINE_MC_ASSET_STORE")
        && !env.trim().is_empty()
    {
        out.push(PathBuf::from(env));
    }
    // W67.x: in test builds, skip the ambient `%APPDATA%/.minecraft`
    // store probe + the offline mirror so tests stay hermetic - they
    // must not pick up the dev box's real asset store (the
    // `F:\Data\mc_assets\asset_store` junction), which would change
    // sound-path resolution counts. The real game (non-test build)
    // keeps the full probe.
    if !cfg!(test) {
        if let Ok(p) = std::env::var("APPDATA")
            && !p.trim().is_empty()
        {
            out.push(PathBuf::from(p).join(".minecraft").join("assets"));
        }
        // POSIX fallback (CI).
        if let Ok(p) = std::env::var("HOME")
            && !p.trim().is_empty()
        {
            out.push(PathBuf::from(p).join(".minecraft").join("assets"));
        }
        // W57: well-known offline mirror this project ships with.
        // Direct hit for the dev machine; harmless on hosts without
        // the directory (load() returns None).
        out.push(PathBuf::from(r"J:\Temp\MC-Sounds\assets"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage_store(root: &Path) {
        let idx = root.join("indexes");
        let obj = root.join("objects").join("aa");
        std::fs::create_dir_all(&idx).unwrap();
        std::fs::create_dir_all(&obj).unwrap();
        let hash = "aabbccddeeff00112233445566778899aabbccdd";
        let target = obj.join(hash);
        std::fs::write(&target, b"hello").unwrap();
        let json = format!(
            r#"{{"objects":{{"foo/bar.ogg":{{"hash":"{hash}","size":5}}}}}}"#
        );
        std::fs::write(idx.join("30.json"), json).unwrap();
    }

    #[test]
    fn load_returns_some_when_layout_valid() {
        let tmp = tempfile::tempdir().unwrap();
        stage_store(tmp.path());
        let store = AssetStore::load(tmp.path()).expect("store");
        assert!(store.index.contains_key("foo/bar.ogg"));
    }

    #[test]
    fn resolve_returns_object_path() {
        let tmp = tempfile::tempdir().unwrap();
        stage_store(tmp.path());
        let store = AssetStore::load(tmp.path()).unwrap();
        let p = store.resolve("foo/bar.ogg").expect("resolved");
        assert!(p.is_file());
    }

    #[test]
    fn resolve_missing_path_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        stage_store(tmp.path());
        let store = AssetStore::load(tmp.path()).unwrap();
        assert!(store.resolve("nope/missing.ogg").is_none());
    }

    #[test]
    fn load_returns_none_without_indexes() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("objects")).unwrap();
        assert!(AssetStore::load(tmp.path()).is_none());
    }

    #[test]
    fn load_picks_highest_numbered_index() {
        // Two indexes; the higher-numbered one wins.
        let tmp = tempfile::tempdir().unwrap();
        stage_store(tmp.path());
        let idx = tmp.path().join("indexes");
        let json = r#"{"objects":{"older.ogg":{"hash":"00112233445566778899aabbccddeeff00112233","size":1}}}"#;
        std::fs::write(idx.join("17.json"), json).unwrap();
        let store = AssetStore::load(tmp.path()).unwrap();
        // 30.json wins -> foo/bar.ogg present, older.ogg absent.
        assert!(store.index.contains_key("foo/bar.ogg"));
        assert!(!store.index.contains_key("older.ogg"));
    }
}
