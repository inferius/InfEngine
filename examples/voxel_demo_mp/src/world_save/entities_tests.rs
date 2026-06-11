//! Integration tests for the per-dimension entity persistence layer
//! (`world_save::entities`).
//!
//! Pulled out of `tests.rs` to keep that file under the 500-LOC soft
//! cap; mirrors the `chunks_tests.rs` + `players_tests.rs` split. Each
//! test spins up a fresh `tempfile::TempDir` and drives the `_in_dir`
//! variants so OS data-dir state never bleeds between runs.
//!
//! Coverage:
//! - Round trips: empty, mobs-only, drops-only, mixed.
//! - Missing file -> Ok(None).
//! - Refusal: unknown envelope version, wrong magic.
//! - Compression toggle: zstd shrinks a repetitive populated payload.
//! - `.bak` recovery: corrupt primary surfaces backup content.

use tempfile::TempDir;

use super::entities::{
    DimensionEntities, DropSave, ENTITIES_FORMAT_VERSION, ENTITIES_MAGIC, MobSave,
    entities_path_in_dir, read_entities_in_dir, write_entities_in_dir,
};

const SLUG: &str = "slot";
const DIM: &str = "overworld";

// Block-id literals used in drop fixtures (independent of any runtime
// `BlockId` mapping - the on-disk schema is just `u16`).
const BLOCK_STONE: u16 = 1;
const BLOCK_DIRT: u16 = 3;
const BLOCK_LOG: u16 = 17;
const BLOCK_PLANK: u16 = 4;
const BLOCK_LEAVES: u16 = 18;

// Mob-kind literals - the on-disk schema is just `u8`. Match the
// likely `MobKind` discriminator order in the runtime so the eventual
// B.7 wiring needs no schema churn.
const KIND_SHEEP: u8 = 0;
const KIND_ZOMBIE: u8 = 1;
const KIND_COW: u8 = 2;

fn sample_sheep(id: u32) -> MobSave {
    MobSave {
        id,
        kind: KIND_SHEEP,
        pos: [10.5, 64.0, -8.25],
        yaw: 0.5,
        vel: [0.1, 0.0, -0.1],
        health: 8.0,
        age_seconds: 120.0,
    }
}

fn sample_zombie(id: u32) -> MobSave {
    MobSave {
        id,
        kind: KIND_ZOMBIE,
        pos: [-3.25, 70.0, 5.75],
        yaw: -1.2,
        vel: [-0.05, -0.2, 0.0],
        health: 20.0,
        age_seconds: 7.5,
    }
}

fn sample_cow(id: u32) -> MobSave {
    MobSave {
        id,
        kind: KIND_COW,
        pos: [0.0, 65.0, 0.0],
        yaw: 0.0,
        vel: [0.0, 0.0, 0.0],
        health: 10.0,
        age_seconds: 600.0,
    }
}

fn sample_drop(id: u32, block: u16) -> DropSave {
    DropSave {
        id,
        block,
        pos: [id as f32 * 0.5, 65.0, id as f32 * -0.25],
        vel: [0.0, 0.1, 0.0],
        age: id as f32 * 1.5,
    }
}

#[test]
fn entities_roundtrip_empty() {
    // A freshly minted empty bundle round trips byte-for-byte through
    // the envelope: encode + zstd-attempt + atomic write + read back.
    let tmp = TempDir::new().unwrap();
    let original = DimensionEntities::empty();
    write_entities_in_dir(tmp.path(), SLUG, DIM, &original).unwrap();

    let loaded = read_entities_in_dir(tmp.path(), SLUG, DIM)
        .unwrap()
        .expect("entities file must exist after write");
    assert_eq!(loaded, original);
    assert_eq!(loaded.format_version, ENTITIES_FORMAT_VERSION);
}

#[test]
fn entities_roundtrip_with_mobs() {
    // Three distinct mob kinds with non-default poses + velocity.
    // Each field has to survive the bincode + zstd hop.
    let tmp = TempDir::new().unwrap();
    let original = DimensionEntities {
        format_version: ENTITIES_FORMAT_VERSION,
        mobs: vec![sample_sheep(0), sample_zombie(1), sample_cow(2)],
        drops: Vec::new(),
        next_mob_id: 3,
        next_drop_id: 0,
        carts: Vec::new(),
        next_cart_id: 0,
    };
    write_entities_in_dir(tmp.path(), SLUG, DIM, &original).unwrap();

    let loaded = read_entities_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded, original);
    assert_eq!(loaded.mobs.len(), 3);
    assert_eq!(loaded.mobs[0].kind, KIND_SHEEP);
    assert_eq!(loaded.mobs[1].kind, KIND_ZOMBIE);
    assert_eq!(loaded.mobs[2].kind, KIND_COW);
    assert_eq!(loaded.next_mob_id, 3);
}

#[test]
fn entities_roundtrip_with_drops() {
    // Five dropped items spanning the block-id space. Drop ages +
    // velocity must survive to feed the despawn-timer derivation on
    // the runtime side.
    let tmp = TempDir::new().unwrap();
    let original = DimensionEntities {
        format_version: ENTITIES_FORMAT_VERSION,
        mobs: Vec::new(),
        drops: vec![
            sample_drop(0, BLOCK_STONE),
            sample_drop(1, BLOCK_DIRT),
            sample_drop(2, BLOCK_LOG),
            sample_drop(3, BLOCK_PLANK),
            sample_drop(4, BLOCK_LEAVES),
        ],
        next_mob_id: 0,
        next_drop_id: 5,
        carts: Vec::new(),
        next_cart_id: 0,
    };
    write_entities_in_dir(tmp.path(), SLUG, DIM, &original).unwrap();

    let loaded = read_entities_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded, original);
    assert_eq!(loaded.drops.len(), 5);
    assert_eq!(loaded.drops[2].block, BLOCK_LOG);
    assert_eq!(loaded.next_drop_id, 5);
}

#[test]
fn entities_roundtrip_mixed() {
    // Both mobs and drops in the same dimension. Realistic shape for
    // an in-progress save.
    let tmp = TempDir::new().unwrap();
    let original = DimensionEntities {
        format_version: ENTITIES_FORMAT_VERSION,
        mobs: vec![sample_sheep(0), sample_zombie(1)],
        drops: vec![sample_drop(0, BLOCK_STONE), sample_drop(1, BLOCK_PLANK)],
        next_mob_id: 2,
        next_drop_id: 2,
        carts: Vec::new(),
        next_cart_id: 0,
    };
    write_entities_in_dir(tmp.path(), SLUG, DIM, &original).unwrap();

    let loaded = read_entities_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded, original);
    assert_eq!(loaded.mobs.len(), 2);
    assert_eq!(loaded.drops.len(), 2);
}

#[test]
fn entities_missing_returns_none() {
    // Server boot treats "no entities file yet" as a fresh dimension.
    // Must not surface that as an io error.
    let tmp = TempDir::new().unwrap();
    let result = read_entities_in_dir(tmp.path(), SLUG, DIM).unwrap();
    assert!(result.is_none());
}

#[test]
fn entities_unknown_version_refuses_read() {
    let tmp = TempDir::new().unwrap();
    write_entities_in_dir(tmp.path(), SLUG, DIM, &DimensionEntities::empty()).unwrap();

    let path = entities_path_in_dir(tmp.path(), SLUG, DIM);
    let mut bytes = std::fs::read(&path).unwrap();
    // Patch the LE u32 at offset 4 to a far-future version (777).
    let bad = 777u32.to_le_bytes();
    bytes[4..8].copy_from_slice(&bad);
    std::fs::write(&path, &bytes).unwrap();
    // Remove the .bak so recovery can't paper over the bad primary.
    let bak = path.with_file_name(format!(
        "{}.bak",
        path.file_name().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_file(&bak);

    let err = read_entities_in_dir(tmp.path(), SLUG, DIM)
        .expect_err("expected envelope version mismatch");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(
        err.to_string().contains("777"),
        "error must mention the unsupported version, got: {err}"
    );
}

#[test]
fn entities_wrong_magic_refuses_read() {
    let tmp = TempDir::new().unwrap();
    write_entities_in_dir(tmp.path(), SLUG, DIM, &DimensionEntities::empty()).unwrap();

    let path = entities_path_in_dir(tmp.path(), SLUG, DIM);
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[..4].copy_from_slice(b"XXXX");
    std::fs::write(&path, &bytes).unwrap();
    // Drop .bak so we don't recover into the test's blind spot.
    let bak = path.with_file_name(format!(
        "{}.bak",
        path.file_name().unwrap().to_string_lossy()
    ));
    let _ = std::fs::remove_file(&bak);

    let err = read_entities_in_dir(tmp.path(), SLUG, DIM)
        .expect_err("expected magic mismatch");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().to_lowercase().contains("magic"));
}

#[test]
fn entities_compression_smaller_payload() {
    // A populated bundle with many similar `MobSave` rows compresses
    // well via zstd. The envelope must pick kind = 1 and the body
    // must end up shorter than the bincode payload.
    let tmp = TempDir::new().unwrap();
    let mut original = DimensionEntities::empty();
    // 64 identical sheep + 64 identical drops - the repetition is the
    // guaranteed zstd win even at level 3.
    for i in 0..64u32 {
        original.mobs.push(sample_sheep(i));
        original.drops.push(sample_drop(i, BLOCK_STONE));
    }
    original.next_mob_id = 64;
    original.next_drop_id = 64;
    write_entities_in_dir(tmp.path(), SLUG, DIM, &original).unwrap();

    let path = entities_path_in_dir(tmp.path(), SLUG, DIM);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..4], ENTITIES_MAGIC);
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    assert_eq!(version, ENTITIES_FORMAT_VERSION);
    assert_eq!(
        bytes[8], 1,
        "repetitive entity bundle should compress with zstd (kind=1)"
    );
    let uncompressed_len = u64::from_le_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        bytes[16],
    ]);
    let body_len = (bytes.len() - 17) as u64;
    assert!(
        body_len < uncompressed_len,
        "zstd body must be shorter than bincode payload: body={body_len}, raw={uncompressed_len}"
    );
    // And it still round trips.
    let loaded = read_entities_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(loaded, original);
}

#[test]
fn entities_recover_from_bak() {
    // Two clean writes -> path = "second", bak = "first". Stomp the
    // live file with garbage. Recovery must surface the .bak content
    // and the next read returns the older (but valid) bundle.
    let tmp = TempDir::new().unwrap();
    let first = DimensionEntities {
        format_version: ENTITIES_FORMAT_VERSION,
        mobs: vec![sample_sheep(0)],
        drops: Vec::new(),
        next_mob_id: 1,
        next_drop_id: 0,
        carts: Vec::new(),
        next_cart_id: 0,
    };
    let second = DimensionEntities {
        format_version: ENTITIES_FORMAT_VERSION,
        mobs: vec![sample_sheep(0), sample_zombie(1)],
        drops: Vec::new(),
        next_mob_id: 2,
        next_drop_id: 0,
        carts: Vec::new(),
        next_cart_id: 0,
    };
    write_entities_in_dir(tmp.path(), SLUG, DIM, &first).unwrap();
    write_entities_in_dir(tmp.path(), SLUG, DIM, &second).unwrap();

    let path = entities_path_in_dir(tmp.path(), SLUG, DIM);
    // Sanity: live file currently parses as `second`.
    let live = read_entities_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(live, second);

    // Stomp the live file with garbage so the primary parse fails.
    std::fs::write(&path, b"junk-not-a-valid-entities-file").unwrap();

    // Recovery surfaces the older `.bak` bundle.
    let recovered = read_entities_in_dir(tmp.path(), SLUG, DIM).unwrap().unwrap();
    assert_eq!(recovered, first, "recovery must surface .bak content");
}
