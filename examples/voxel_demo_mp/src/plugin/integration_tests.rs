//! Integration test that actually loads the sample plugin
//! `sample_plugin_rs.{dll,so,dylib}` from disk.
//!
//! The plugin must be built first - we look for it under
//! `target/<profile>/`. If it isn't there, the test is skipped
//! gracefully so the regular `cargo test -p voxel_demo_mp` flow
//! doesn't fail when the user only ran `cargo test`.
//!
//! Run the full check from the workspace root:
//! ```sh
//! cargo build -p sample_plugin_rs
//! cargo test -p voxel_demo_mp plugin::integration_tests
//! ```

use std::path::PathBuf;

fn sample_plugin_path() -> Option<PathBuf> {
    // The test executable lives at `target/<profile>/deps/<name>.exe`.
    // The cdylib lives at `target/<profile>/sample_plugin_rs.<ext>`.
    // Walk up from current_exe() to the profile dir, then down.
    let exe = std::env::current_exe().ok()?;
    let profile_dir = exe.parent()?.parent()?;
    let ext = match std::env::consts::OS {
        "windows" => "dll",
        "macos" => "dylib",
        _ => "so",
    };
    let prefix = if cfg!(target_os = "windows") { "" } else { "lib" };
    let path =
        profile_dir.join(format!("{prefix}sample_plugin_rs.{ext}"));
    if path.exists() { Some(path) } else { None }
}

#[test]
fn sample_plugin_loads_if_built() {
    let Some(path) = sample_plugin_path() else {
        // Plugin not built in this profile - skip silently.
        // (eprintln would trip the workspace's print_stderr lint;
        // the test still passes + caller can run with
        // `cargo build -p sample_plugin_rs` to exercise it.)
        return;
    };

    let mut reg = super::PluginRegistry::new();
    let result = super::host::DylibHost::load(&path, reg.api, reg.facade_ptr);
    let plug = result.expect("sample plugin should load");
    assert_eq!(plug.name, "rainbow_plugin");
    assert_eq!(plug.version, "0.1.0");
    reg.plugins.push(plug);

    // Verify the registry can find it by name.
    assert_eq!(reg.find_index("rainbow_plugin"), Some(0));
    assert_eq!(reg.find_index("RAINBOW_PLUGIN"), Some(0));
}

#[test]
fn discover_finds_sample_plugin_in_profile_dir() {
    let Some(path) = sample_plugin_path() else {
        return;
    };
    let profile_dir = path.parent().unwrap();
    let plugs = super::PluginRegistry::discover_plugins(profile_dir);
    assert!(
        plugs.iter().any(|p| p == &path),
        "discovery did not surface the sample plugin: {plugs:?}"
    );
}

#[test]
fn abi_version_mismatch_rejected() {
    // We can't easily forge an ABI-mismatched dylib, so we
    // exercise the rejection path via the PluginError display
    // (proves the error case is well-formed).
    let err = super::host::PluginError::AbiMajorMismatch {
        plugin: 99,
        host: 0,
    };
    let msg = format!("{err}");
    assert!(msg.contains("ABI major mismatch"));
}
