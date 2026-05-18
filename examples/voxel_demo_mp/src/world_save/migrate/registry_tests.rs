//! Unit tests for the bare `MigrationRegistry` mechanics + the peek
//! helpers + the error sidecar path generator.
//!
//! Co-located with the source (rather than in the world_save-level
//! `migrate_tests.rs`) so the cases that need access to private items
//! like `error_sidecar_path` keep working without widening the API
//! surface. End-to-end migration tests through the six reader paths
//! live in `world_save::migrate_tests`.

use std::io;
use std::path::Path;

use super::{
    MigrationRegistry, SaveKind, default_registry, error_sidecar_path, peek_envelope_version,
    peek_json_version,
};

/// Trivial step that prepends a single byte. Lets us tell which arrow
/// ran by inspecting the output.
fn prepend_a(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut out = vec![b'A'];
    out.extend_from_slice(bytes);
    Ok(out)
}

fn prepend_b(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut out = vec![b'B'];
    out.extend_from_slice(bytes);
    Ok(out)
}

fn prepend_c(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let mut out = vec![b'C'];
    out.extend_from_slice(bytes);
    Ok(out)
}

#[test]
fn empty_registry_returns_none_for_any_version() {
    let reg = MigrationRegistry::new();
    assert!(reg.plan(SaveKind::Meta, 1, 2).is_none());
    assert!(reg.plan(SaveKind::Chunk, 5, 7).is_none());
}

#[test]
fn empty_registry_passes_through_when_versions_match() {
    let reg = MigrationRegistry::new();
    let plan = reg.plan(SaveKind::Meta, 3, 3).unwrap();
    assert!(plan.is_empty());
}

#[test]
fn register_single_step_then_plan_finds_it() {
    let mut reg = MigrationRegistry::new();
    reg.register(SaveKind::Meta, 1, 2, prepend_a);
    let plan = reg.plan(SaveKind::Meta, 1, 2).unwrap();
    assert_eq!(plan.len(), 1);
}

#[test]
fn register_two_steps_then_plan_chains_them() {
    let mut reg = MigrationRegistry::new();
    reg.register(SaveKind::Player, 1, 2, prepend_a);
    reg.register(SaveKind::Player, 2, 3, prepend_b);
    let plan = reg.plan(SaveKind::Player, 1, 3).unwrap();
    assert_eq!(plan.len(), 2);
}

#[test]
fn plan_no_path_returns_none() {
    let mut reg = MigrationRegistry::new();
    // v1 -> v2 registered, but target is v3 with no v2 -> v3.
    reg.register(SaveKind::Chunk, 1, 2, prepend_a);
    assert!(reg.plan(SaveKind::Chunk, 1, 3).is_none());
}

#[test]
fn plan_rejects_downgrade() {
    let mut reg = MigrationRegistry::new();
    reg.register(SaveKind::Meta, 1, 2, prepend_a);
    // Asking to go from v2 back to v1 has no chain.
    assert!(reg.plan(SaveKind::Meta, 2, 1).is_none());
}

#[test]
fn plan_isolates_kinds() {
    let mut reg = MigrationRegistry::new();
    reg.register(SaveKind::Meta, 1, 2, prepend_a);
    // Same versions but different kind -> no chain.
    assert!(reg.plan(SaveKind::Chunk, 1, 2).is_none());
}

#[test]
fn apply_runs_steps_in_order() {
    let mut reg = MigrationRegistry::new();
    reg.register(SaveKind::Meta, 1, 2, prepend_a);
    reg.register(SaveKind::Meta, 2, 3, prepend_b);
    let out = reg.apply(SaveKind::Meta, 1, 3, b"x").unwrap();
    // Order: A then B -> "BAx" (B is the last prepend so it sits at
    // the head).
    assert_eq!(out, b"BAx");
}

#[test]
fn apply_passes_payload_between_steps() {
    let mut reg = MigrationRegistry::new();
    reg.register(SaveKind::Meta, 1, 2, prepend_a);
    reg.register(SaveKind::Meta, 2, 3, prepend_b);
    reg.register(SaveKind::Meta, 3, 4, prepend_c);
    let out = reg.apply(SaveKind::Meta, 1, 4, b"z").unwrap();
    assert_eq!(out, b"CBAz");
}

#[test]
fn apply_no_chain_errors() {
    let reg = MigrationRegistry::new();
    let err = reg.apply(SaveKind::Meta, 1, 2, b"x").unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn peek_json_version_finds_format_version() {
    let bytes = br#"{"format_version": 7, "other": "ignored"}"#;
    assert_eq!(peek_json_version(bytes).unwrap(), 7);
}

#[test]
fn peek_json_version_missing_field_errors() {
    let bytes = br#"{"other": "no version"}"#;
    let err = peek_json_version(bytes).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn peek_envelope_version_reads_offset_4() {
    // 4 bytes magic + 4 bytes LE u32 version = 8 bytes minimum.
    let mut bytes = b"INFC".to_vec();
    bytes.extend_from_slice(&42u32.to_le_bytes());
    assert_eq!(peek_envelope_version(&bytes).unwrap(), 42);
}

#[test]
fn peek_envelope_version_short_input_errors() {
    let err = peek_envelope_version(b"INFC").unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn default_registry_is_empty_at_phase_b6() {
    // Locks the "no production migrations yet" state. Bumping any
    // payload version is a deliberate breaking change that also adds
    // an arrow.
    let reg = default_registry();
    assert!(reg.plan(SaveKind::Meta, 1, 2).is_none());
    assert!(reg.plan(SaveKind::WorldJson, 1, 2).is_none());
    assert!(reg.plan(SaveKind::Chunk, 1, 2).is_none());
    assert!(reg.plan(SaveKind::Player, 1, 2).is_none());
    assert!(reg.plan(SaveKind::Entities, 1, 2).is_none());
    assert!(reg.plan(SaveKind::Timers, 1, 2).is_none());
}

#[test]
fn error_sidecar_path_has_timestamp() {
    let p = error_sidecar_path(Path::new("meta.json"));
    let s = p.to_string_lossy().to_string();
    assert!(
        s.starts_with("meta.json.error."),
        "sidecar must extend original path, got: {s}"
    );
    // Suffix: 8 digits + dash + 6 digits + dash + 3 digits = 19 chars
    // after the dot.
    assert!(s.len() > "meta.json.error.".len() + 18);
}
