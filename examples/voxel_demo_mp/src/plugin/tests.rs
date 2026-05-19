//! Unit tests for the plugin host. We exercise the discovery +
//! load + dispatch paths without spinning up a real dylib by
//! constructing a `LoadedPlugin` in-place with hand-rolled
//! `extern "C"` callbacks. The real-dylib integration test
//! lives in `plugin::integration_tests`.

use std::ffi::CString;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::OnceLock;

use infengine_sdk::{
    ABI_MAJOR, ABI_MINOR, BlockEvent, ChatEvent, EngineHandle, PluginInfo,
    PluginVTable, TickEvent,
};

use super::api_impl::EngineFacade;
use super::host::{self, LoadedPlugin, PluginError};

// ---------------------------------------------------------------
// Counter-based extern "C" callbacks shared by the tests below.
// ---------------------------------------------------------------

static TICKS: AtomicUsize = AtomicUsize::new(0);
static PLACES: AtomicUsize = AtomicUsize::new(0);
static BREAKS: AtomicUsize = AtomicUsize::new(0);
static CHATS: AtomicUsize = AtomicUsize::new(0);
static INIT_CALLED: AtomicUsize = AtomicUsize::new(0);
static SHUTDOWN_CALLED: AtomicUsize = AtomicUsize::new(0);
static LAST_PLAYER: AtomicU32 = AtomicU32::new(0);
static LAST_BLOCK: AtomicU32 = AtomicU32::new(0);
static PANIC_NEXT: AtomicUsize = AtomicUsize::new(0);

fn reset_counters() {
    TICKS.store(0, Ordering::Relaxed);
    PLACES.store(0, Ordering::Relaxed);
    BREAKS.store(0, Ordering::Relaxed);
    CHATS.store(0, Ordering::Relaxed);
    INIT_CALLED.store(0, Ordering::Relaxed);
    SHUTDOWN_CALLED.store(0, Ordering::Relaxed);
    LAST_PLAYER.store(0, Ordering::Relaxed);
    LAST_BLOCK.store(0, Ordering::Relaxed);
    PANIC_NEXT.store(0, Ordering::Relaxed);
}

unsafe extern "C-unwind" fn cb_init(_h: *mut EngineHandle) {
    INIT_CALLED.fetch_add(1, Ordering::Relaxed);
}

unsafe extern "C-unwind" fn cb_shutdown(_h: *mut EngineHandle) {
    SHUTDOWN_CALLED.fetch_add(1, Ordering::Relaxed);
}

unsafe extern "C-unwind" fn cb_tick(_h: *mut EngineHandle, _ev: TickEvent) {
    if PANIC_NEXT.load(Ordering::Relaxed) > 0 {
        PANIC_NEXT.fetch_sub(1, Ordering::Relaxed);
        panic!("test-induced panic in on_tick");
    }
    TICKS.fetch_add(1, Ordering::Relaxed);
}

unsafe extern "C-unwind" fn cb_place(_h: *mut EngineHandle, ev: BlockEvent) {
    PLACES.fetch_add(1, Ordering::Relaxed);
    LAST_BLOCK.store(ev.block_id as u32, Ordering::Relaxed);
    LAST_PLAYER.store(ev.player_id, Ordering::Relaxed);
}

unsafe extern "C-unwind" fn cb_break(_h: *mut EngineHandle, ev: BlockEvent) {
    BREAKS.fetch_add(1, Ordering::Relaxed);
    LAST_BLOCK.store(ev.block_id as u32, Ordering::Relaxed);
}

unsafe extern "C-unwind" fn cb_chat(_h: *mut EngineHandle, ev: ChatEvent) {
    CHATS.fetch_add(1, Ordering::Relaxed);
    LAST_PLAYER.store(ev.speaker_id, Ordering::Relaxed);
}

/// Build a `LoadedPlugin` directly without going through a real
/// dylib. We grab a self-handle to the engine binary as a
/// libloading::Library placeholder so the `lib` field is valid
/// (we only need the field; we never call into it).
fn make_synthetic_plugin(
    name: &str,
    vtable: PluginVTable,
) -> LoadedPlugin {
    // We need a `libloading::Library` for the struct. Re-open
    // a library we know is always present on the running
    // process. On Windows this is "kernel32.dll"; on Unix "libc"
    // (or empty path for the main image). We never actually
    // dispatch through this Library; the fn pointers were
    // baked in by us above.
    let lib = open_placeholder_lib();
    LoadedPlugin {
        path: std::path::PathBuf::from(format!("synthetic://{name}")),
        name: name.to_string(),
        version: "0.0.0".to_string(),
        disabled: false,
        disable_reason: None,
        vtable,
        lib,
    }
}

fn open_placeholder_lib() -> libloading::Library {
    static LIB: OnceLock<usize> = OnceLock::new();
    // We can't return a borrow across the OnceLock boundary
    // because Library isn't Clone. Re-open on each call - the
    // OS de-dupes the handle internally. We use OnceLock just
    // as a sanity check that opening succeeded once.
    let lib = if cfg!(windows) {
        // SAFETY: opening a known-good system DLL for the
        // duration of this test; we never resolve symbols.
        unsafe { libloading::Library::new("kernel32.dll") }
    } else if cfg!(target_os = "macos") {
        // SAFETY: same as above.
        unsafe { libloading::Library::new("libc.dylib") }
    } else {
        // SAFETY: same as above.
        unsafe { libloading::Library::new("libc.so.6") }
    };
    let lib = lib.expect("placeholder dynamic library must open");
    LIB.set(1).ok();
    lib
}

fn make_facade() -> *mut EngineFacade {
    Box::into_raw(Box::new(EngineFacade {
        voxel: std::ptr::null_mut(),
        block_register_budget_next: 1000,
    }))
}

fn free_facade(p: *mut EngineFacade) {
    if p.is_null() {
        return;
    }
    // SAFETY: came from Box::into_raw above.
    unsafe { drop(Box::from_raw(p)) };
}

// ---------------------------------------------------------------
// Tests
// ---------------------------------------------------------------

#[test]
fn discover_returns_empty_for_missing_dir() {
    let tmp = std::env::temp_dir().join("infengine_no_such_dir_for_test");
    let _ = std::fs::remove_dir_all(&tmp);
    let plugs = super::PluginRegistry::discover_plugins(&tmp);
    assert!(plugs.is_empty());
}

#[test]
fn discover_filters_by_platform_extension() {
    let tmp = std::env::temp_dir().join("infengine_plugin_discovery_test");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    // Drop one matching file + one non-matching one. We don't
    // need them to be valid libraries; discovery only checks
    // the extension.
    let ext = match std::env::consts::OS {
        "windows" => "dll",
        "macos" => "dylib",
        _ => "so",
    };
    let good = tmp.join(format!("foo.{ext}"));
    let bad = tmp.join("README.txt");
    std::fs::write(&good, b"").unwrap();
    std::fs::write(&bad, b"").unwrap();
    let plugs = super::PluginRegistry::discover_plugins(&tmp);
    assert_eq!(plugs.len(), 1);
    assert_eq!(plugs[0].file_name(), good.file_name());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn discover_dir_env_override_wins() {
    let tmp = std::env::temp_dir().join("infengine_plugin_env_override");
    let _ = std::fs::remove_dir_all(&tmp);
    // SAFETY: tests in this crate run on one thread per test
    // by default - env-var mutation is fine.
    unsafe { std::env::set_var(super::ENV_PLUGIN_DIR, &tmp) };
    let resolved = super::PluginRegistry::discover_dir();
    assert_eq!(resolved, tmp);
    // SAFETY: same as above.
    unsafe { std::env::remove_var(super::ENV_PLUGIN_DIR) };
}

#[test]
fn dispatch_tick_increments_counter() {
    reset_counters();
    let vt = PluginVTable {
        on_tick: Some(cb_tick),
        ..PluginVTable::default()
    };
    let mut plug = make_synthetic_plugin("tick_plug", vt);
    let facade = make_facade();
    let dim = CString::new("overworld").unwrap();
    host::dispatch_tick(&mut plug, facade, 0.016, 0.5, &dim);
    host::dispatch_tick(&mut plug, facade, 0.016, 0.5, &dim);
    free_facade(facade);
    assert_eq!(TICKS.load(Ordering::Relaxed), 2);
}

#[test]
fn dispatch_block_event_passes_payload() {
    reset_counters();
    let vt = PluginVTable {
        on_block_place: Some(cb_place),
        on_block_break: Some(cb_break),
        ..PluginVTable::default()
    };
    let mut plug = make_synthetic_plugin("blk", vt);
    let facade = make_facade();
    host::dispatch_block_event(&mut plug, facade, true, 1, 2, 3, 42, 7);
    host::dispatch_block_event(&mut plug, facade, false, 1, 2, 3, 0, 7);
    free_facade(facade);
    assert_eq!(PLACES.load(Ordering::Relaxed), 1);
    assert_eq!(BREAKS.load(Ordering::Relaxed), 1);
    assert_eq!(LAST_PLAYER.load(Ordering::Relaxed), 7);
}

#[test]
fn dispatch_chat_passes_speaker() {
    reset_counters();
    let vt = PluginVTable {
        on_chat_msg: Some(cb_chat),
        ..PluginVTable::default()
    };
    let mut plug = make_synthetic_plugin("chat", vt);
    let facade = make_facade();
    let msg = CString::new("hello").unwrap();
    host::dispatch_chat(&mut plug, facade, 99, &msg);
    free_facade(facade);
    assert_eq!(CHATS.load(Ordering::Relaxed), 1);
    assert_eq!(LAST_PLAYER.load(Ordering::Relaxed), 99);
}

#[test]
fn dispatch_skips_disabled_plugin() {
    reset_counters();
    let vt = PluginVTable {
        on_tick: Some(cb_tick),
        ..PluginVTable::default()
    };
    let mut plug = make_synthetic_plugin("disabled", vt);
    plug.disable("test");
    let facade = make_facade();
    let dim = CString::new("overworld").unwrap();
    host::dispatch_tick(&mut plug, facade, 0.016, 0.5, &dim);
    free_facade(facade);
    assert_eq!(TICKS.load(Ordering::Relaxed), 0);
    assert!(plug.disabled);
}

#[test]
fn dispatch_skips_none_vtable_slot() {
    reset_counters();
    let mut plug = make_synthetic_plugin("empty", PluginVTable::default());
    let facade = make_facade();
    let dim = CString::new("overworld").unwrap();
    host::dispatch_tick(&mut plug, facade, 0.016, 0.5, &dim);
    host::dispatch_block_event(&mut plug, facade, true, 0, 0, 0, 0, 0);
    host::dispatch_chat(
        &mut plug,
        facade,
        0,
        &CString::new("x").unwrap(),
    );
    free_facade(facade);
    // No callbacks fired - all counters zero.
    assert_eq!(TICKS.load(Ordering::Relaxed), 0);
    assert_eq!(PLACES.load(Ordering::Relaxed), 0);
    assert_eq!(CHATS.load(Ordering::Relaxed), 0);
    // Plugin is NOT auto-disabled by an absent slot.
    assert!(!plug.disabled);
}

#[test]
fn dispatch_disables_plugin_on_panic() {
    reset_counters();
    let vt = PluginVTable {
        on_tick: Some(cb_tick),
        ..PluginVTable::default()
    };
    let mut plug = make_synthetic_plugin("paniky", vt);
    let facade = make_facade();
    let dim = CString::new("overworld").unwrap();
    PANIC_NEXT.store(1, Ordering::Relaxed);
    host::dispatch_tick(&mut plug, facade, 0.016, 0.5, &dim);
    assert!(plug.disabled);
    assert!(plug.disable_reason.as_deref().unwrap().contains("panic"));
    // Subsequent calls no-op (plugin is disabled).
    host::dispatch_tick(&mut plug, facade, 0.016, 0.5, &dim);
    free_facade(facade);
    assert_eq!(TICKS.load(Ordering::Relaxed), 0);
}

#[test]
fn loaded_plugin_shutdown_safely_runs_callback() {
    reset_counters();
    let vt = PluginVTable {
        on_shutdown: Some(cb_shutdown),
        ..PluginVTable::default()
    };
    let mut plug = make_synthetic_plugin("shutdown", vt);
    let facade = make_facade();
    plug.shutdown_safely(facade);
    free_facade(facade);
    assert_eq!(SHUTDOWN_CALLED.load(Ordering::Relaxed), 1);
}

#[test]
fn plugin_error_display_includes_versions() {
    let e = PluginError::AbiMajorMismatch { plugin: 5, host: 0 };
    let s = format!("{e}");
    assert!(s.contains("plugin=5"));
    assert!(s.contains("host=0"));
}

#[test]
fn registry_new_yields_empty_list() {
    let reg = super::PluginRegistry::new();
    assert!(reg.plugins.is_empty());
}

#[test]
fn registry_find_index_is_case_insensitive() {
    let mut reg = super::PluginRegistry::new();
    let vt = PluginVTable {
        on_init: Some(cb_init),
        ..PluginVTable::default()
    };
    reg.plugins.push(make_synthetic_plugin("MyPlug", vt));
    assert_eq!(reg.find_index("myplug"), Some(0));
    assert_eq!(reg.find_index("MYPLUG"), Some(0));
    assert_eq!(reg.find_index("missing"), None);
}

#[test]
fn registry_reload_index_out_of_range_errors() {
    let mut reg = super::PluginRegistry::new();
    let err = reg.reload(99).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("out of range"));
}

#[test]
fn abi_constants_round_trip_into_plugin_info() {
    // Make sure a PluginInfo we construct manually with the
    // current ABI constants matches our compiled view.
    let name = c"x";
    let version = c"0";
    let info = PluginInfo {
        abi_major: ABI_MAJOR,
        abi_minor: ABI_MINOR,
        name: name.as_ptr(),
        version: version.as_ptr(),
    };
    assert_eq!(info.abi_major, ABI_MAJOR);
    assert_eq!(info.abi_minor, ABI_MINOR);
    let _ = info.name;
    let _ = info.version;
}

#[test]
fn engine_api_table_has_all_six_slots() {
    // Smoke: building the API doesn't trip OOM / unsafe issues.
    // Function pointers in a `#[repr(C)]` struct are non-null
    // by construction (Rust fn ptrs are non-nullable), so we
    // just verify the table builds + sizes correctly.
    let api = super::api_impl::build_engine_api();
    assert_eq!(
        std::mem::size_of_val(&api),
        6 * std::mem::size_of::<*const ()>()
    );
}
