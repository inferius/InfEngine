//! Shared test-only environment guard for the resource_pack module.
//!
//! `std::env::set_var` is unsafe in multi-threaded code (other
//! threads may read the same key) and cargo runs tests in parallel,
//! so every env-mutating test in this module (the `paths` discovery
//! tests + the integration tests) serialises through the one
//! `ENV_LOCK` here. Using a single shared lock - rather than one per
//! test module - keeps those modules from racing each other.
//!
//! `force_placeholder` neutralises an ambient MC install (the
//! `INFENGINE_MC_ASSETS` / `_STORE` env vars *and* a real
//! `%APPDATA%/.minecraft` tree - e.g. the dev-box junctions that point
//! at `F:\Data\mc_assets`) so asset discovery deterministically falls
//! back to the placeholder pack, the way the pack tests assume.

use std::sync::Mutex;

/// Global mutex serialising every env-mutating test in this module.
pub(crate) static ENV_LOCK: Mutex<()> = Mutex::new(());

/// RAII multi-key env guard. Holds [`ENV_LOCK`] for its lifetime and
/// restores every mutated key on drop, so parallel tests stay fenced.
pub(crate) struct EnvScope {
    prev: Vec<(&'static str, Option<String>)>,
    _g: std::sync::MutexGuard<'static, ()>,
}

impl EnvScope {
    pub(crate) fn new() -> Self {
        let g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        Self {
            prev: Vec::new(),
            _g: g,
        }
    }

    pub(crate) fn set(&mut self, key: &'static str, val: &str) -> &mut Self {
        let prev = std::env::var(key).ok();
        // SAFETY: ENV_LOCK serializes all env mutation in this module.
        unsafe { std::env::set_var(key, val) };
        self.prev.push((key, prev));
        self
    }

    pub(crate) fn unset(&mut self, key: &'static str) -> &mut Self {
        let prev = std::env::var(key).ok();
        // SAFETY: ENV_LOCK serializes all env mutation in this module.
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

/// Force MC asset discovery to find nothing (-> placeholder pack):
/// clears the override env vars and points `APPDATA` / `HOME` at an
/// empty temp dir so the `.minecraft` probe (including a dev-box
/// junction) misses. Hold BOTH returned values for the whole test
/// body - the `EnvScope` keeps the lock + env override live, the
/// `TempDir` keeps the empty APPDATA on disk.
pub(crate) fn force_placeholder() -> (EnvScope, tempfile::TempDir) {
    let tmp = tempfile::tempdir().unwrap();
    let p = tmp.path().to_str().unwrap().to_string();
    let mut env = EnvScope::new();
    env.set("INFENGINE_MC_ASSETS", "")
        .set("INFENGINE_MC_ASSET_STORE", "")
        .set("APPDATA", &p)
        .set("HOME", &p);
    (env, tmp)
}
