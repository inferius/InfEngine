//! Integration tests for the per-chunk persistence layer
//! (`world_save::chunks`).
//!
//! Pulled out of `tests.rs` to keep that file under the 500-LOC soft
//! cap. Each test spins up a fresh `tempfile::TempDir` and drives the
//! `_in_dir` variants so OS data-dir state never bleeds between runs.
//!
//! Coverage:
//! - Round trips: uniform-air, sparse, chequerboard, liquid levels,
//!   torch + snow decorations.
//! - Missing file -> Ok(None).
//! - Compression toggle: zstd when smaller, raw fallback otherwise.
//! - Refusal: unknown envelope version, wrong magic.
//! - Footprint sanity check.

use infengine_voxel::{BlockId, CHUNK_SIZE, Chunk};
use tempfile::TempDir;

use super::chunks::{
    CHUNK_FORMAT_VERSION, CHUNK_MAGIC, CHUNK_PAYLOAD_VERSION, ChunkPayload, ChunkVoxels,
    chunk_path_in_dir, empty_payload, read_chunk_in_dir, write_chunk_in_dir,
};

/// Helper: build a chequerboard chunk - alternating stone+air. This
/// is the RLE worst case (every cell flips the run), but the round
/// trip must still produce a byte-identical decoded chunk.
fn chequerboard_chunk() -> Chunk {
    let mut chunk = Chunk::empty();
    for z in 0..CHUNK_SIZE {
        for y in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                if (x + y + z) % 2 == 0 {
                    chunk.set(x, y, z, BlockId(1));
                }
            }
        }
    }
    chunk
}

fn assert_chunks_eq(a: &Chunk, b: &Chunk) {
    for z in 0..CHUNK_SIZE {
        for y in 0..CHUNK_SIZE {
            for x in 0..CHUNK_SIZE {
                assert_eq!(
                    a.get(x, y, z),
                    b.get(x, y, z),
                    "voxel mismatch at ({x}, {y}, {z})"
                );
            }
        }
    }
}

#[test]
fn chunk_roundtrip_air() {
    // Uniformly-air chunk: one RLE run, full round trip restores
    // an identical chunk.
    let tmp = TempDir::new().unwrap();
    let payload = empty_payload([0, 0, 0]);
    assert_eq!(
        payload.voxels.runs.len(),
        1,
        "uniform air must encode as a single run"
    );

    write_chunk_in_dir(tmp.path(), "slot", "overworld", &payload).unwrap();
    let loaded = read_chunk_in_dir(tmp.path(), "slot", "overworld", [0, 0, 0])
        .unwrap()
        .expect("chunk file must exist after write");
    assert_eq!(loaded, payload);
    let chunk = loaded.voxels.into_chunk().unwrap();
    assert_chunks_eq(&chunk, &Chunk::empty());
}

#[test]
fn chunk_roundtrip_one_block() {
    // One stone block in an otherwise empty chunk. Decoded data
    // must match cell-for-cell at the exact coord we wrote.
    let tmp = TempDir::new().unwrap();
    let mut chunk = Chunk::empty();
    chunk.set(5, 5, 5, BlockId(1));
    let payload = ChunkPayload {
        format_version: CHUNK_PAYLOAD_VERSION,
        coord: [3, -1, 7],
        voxels: ChunkVoxels::from_chunk(&chunk),
        liquid_levels: Vec::new(),
        torch_orient: Vec::new(),
        snow_layers: Vec::new(),
    };

    write_chunk_in_dir(tmp.path(), "slot", "overworld", &payload).unwrap();
    let loaded = read_chunk_in_dir(tmp.path(), "slot", "overworld", [3, -1, 7])
        .unwrap()
        .unwrap();
    assert_eq!(loaded.coord, [3, -1, 7]);
    let decoded = loaded.voxels.into_chunk().unwrap();
    assert_chunks_eq(&decoded, &chunk);
    assert_eq!(decoded.get(5, 5, 5), BlockId(1));
}

#[test]
fn chunk_roundtrip_chequerboard() {
    // RLE worst case: alternating stone+air. Run count is in the
    // 4k range; round trip must still match every cell.
    let tmp = TempDir::new().unwrap();
    let chunk = chequerboard_chunk();
    let payload = ChunkPayload {
        format_version: CHUNK_PAYLOAD_VERSION,
        coord: [0, 0, 0],
        voxels: ChunkVoxels::from_chunk(&chunk),
        liquid_levels: Vec::new(),
        torch_orient: Vec::new(),
        snow_layers: Vec::new(),
    };
    // Sanity-check the encoder hit (close to) the worst case so
    // future refactors of `from_chunk` don't silently regress it.
    assert!(
        payload.voxels.runs.len() >= 3000,
        "chequerboard should produce thousands of runs, got {}",
        payload.voxels.runs.len()
    );

    write_chunk_in_dir(tmp.path(), "slot", "overworld", &payload).unwrap();
    let loaded = read_chunk_in_dir(tmp.path(), "slot", "overworld", [0, 0, 0])
        .unwrap()
        .unwrap();
    let decoded = loaded.voxels.into_chunk().unwrap();
    assert_chunks_eq(&decoded, &chunk);
}

#[test]
fn chunk_roundtrip_with_liquid_levels() {
    let tmp = TempDir::new().unwrap();
    let mut payload = empty_payload([0, 0, 0]);
    payload.liquid_levels =
        vec![([1, 2, 3], 7), ([4, 5, 6], 3), ([15, 0, 15], 1)];

    write_chunk_in_dir(tmp.path(), "slot", "overworld", &payload).unwrap();
    let loaded = read_chunk_in_dir(tmp.path(), "slot", "overworld", [0, 0, 0])
        .unwrap()
        .unwrap();
    assert_eq!(loaded.liquid_levels, payload.liquid_levels);
}

#[test]
fn chunk_roundtrip_with_decorations() {
    let tmp = TempDir::new().unwrap();
    let mut payload = empty_payload([1, 1, 1]);
    payload.torch_orient = vec![([2, 3, 4], 1), ([5, 6, 7], 2)];
    payload.snow_layers = vec![([0, 0, 0], 1), ([1, 0, 0], 4), ([2, 0, 0], 8)];

    write_chunk_in_dir(tmp.path(), "slot", "overworld", &payload).unwrap();
    let loaded = read_chunk_in_dir(tmp.path(), "slot", "overworld", [1, 1, 1])
        .unwrap()
        .unwrap();
    assert_eq!(loaded.torch_orient, payload.torch_orient);
    assert_eq!(loaded.snow_layers, payload.snow_layers);
}

#[test]
fn chunk_missing_returns_none() {
    // Streaming layer treats a missing chunk as "regenerate from
    // worldgen". Must not be an error.
    let tmp = TempDir::new().unwrap();
    let result =
        read_chunk_in_dir(tmp.path(), "slot", "overworld", [42, 42, 42]).unwrap();
    assert!(result.is_none());
}

#[test]
fn chunk_compression_uses_zstd_when_smaller() {
    // A chequerboard chunk produces a huge bincode payload (thousands
    // of runs, each (u16, u16) = ~12 KB raw) with highly repetitive
    // byte content - the ideal case for zstd. The encoder must pick
    // the compressed branch (kind = 1).
    let tmp = TempDir::new().unwrap();
    let chunk = chequerboard_chunk();
    let payload = ChunkPayload {
        format_version: CHUNK_PAYLOAD_VERSION,
        coord: [0, 0, 0],
        voxels: ChunkVoxels::from_chunk(&chunk),
        liquid_levels: Vec::new(),
        torch_orient: Vec::new(),
        snow_layers: Vec::new(),
    };
    write_chunk_in_dir(tmp.path(), "slot", "overworld", &payload).unwrap();

    let path = chunk_path_in_dir(tmp.path(), "slot", "overworld", [0, 0, 0]);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..4], CHUNK_MAGIC);
    let version = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    assert_eq!(version, CHUNK_FORMAT_VERSION);
    assert_eq!(
        bytes[8], 1,
        "highly compressible chunk should compress with zstd (kind=1)"
    );
    // On-disk body length must be shorter than the bincode payload
    // length the envelope claims - confirms we wrote the zstd bytes,
    // not the raw fallback.
    let uncompressed_len = u64::from_le_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        bytes[16],
    ]);
    let body_len = (bytes.len() - 17) as u64;
    assert!(
        body_len < uncompressed_len,
        "zstd body must be shorter than bincode payload: body={body_len}, raw={uncompressed_len}"
    );
}

#[test]
fn chunk_compression_skips_zstd_when_larger() {
    // Force the raw-fallback path: a uniformly-air chunk encodes as
    // a single-run payload (~14 bytes bincode). zstd's frame header
    // alone is bigger than the input, so the encoder must pick
    // kind = 0 to avoid inflating the file.
    let tmp = TempDir::new().unwrap();
    let payload = empty_payload([9, 9, 9]);
    write_chunk_in_dir(tmp.path(), "slot", "overworld", &payload).unwrap();

    let path = chunk_path_in_dir(tmp.path(), "slot", "overworld", [9, 9, 9]);
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(
        bytes[8], 0,
        "tiny payload must skip zstd to avoid header overhead"
    );
    // Body length should equal the uncompressed_payload_len field
    // when kind = 0 - sanity check the envelope layout.
    let uncompressed_len = u64::from_le_bytes([
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        bytes[16],
    ]) as usize;
    assert_eq!(bytes.len() - 17, uncompressed_len);
    // Round trip still works.
    let loaded = read_chunk_in_dir(tmp.path(), "slot", "overworld", [9, 9, 9])
        .unwrap()
        .unwrap();
    assert_eq!(loaded, payload);
}

#[test]
fn chunk_unknown_header_version_refuses_read() {
    // Corrupt the envelope format_version byte. Read must fail with
    // an `InvalidData` error that mentions the bad version.
    let tmp = TempDir::new().unwrap();
    let payload = empty_payload([0, 0, 0]);
    write_chunk_in_dir(tmp.path(), "slot", "overworld", &payload).unwrap();

    let path = chunk_path_in_dir(tmp.path(), "slot", "overworld", [0, 0, 0]);
    let mut bytes = std::fs::read(&path).unwrap();
    // Patch the LE u32 at offset 4 to a far-future version (777).
    let bad = 777u32.to_le_bytes();
    bytes[4..8].copy_from_slice(&bad);
    std::fs::write(&path, &bytes).unwrap();

    let err = read_chunk_in_dir(tmp.path(), "slot", "overworld", [0, 0, 0])
        .expect_err("expected envelope version mismatch");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(
        err.to_string().contains("777"),
        "error must mention the unsupported version, got: {err}"
    );
}

#[test]
fn chunk_wrong_magic_refuses_read() {
    let tmp = TempDir::new().unwrap();
    let payload = empty_payload([0, 0, 0]);
    write_chunk_in_dir(tmp.path(), "slot", "overworld", &payload).unwrap();

    let path = chunk_path_in_dir(tmp.path(), "slot", "overworld", [0, 0, 0]);
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[..4].copy_from_slice(b"XXXX");
    std::fs::write(&path, &bytes).unwrap();

    let err = read_chunk_in_dir(tmp.path(), "slot", "overworld", [0, 0, 0])
        .expect_err("expected magic mismatch");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    assert!(err.to_string().to_lowercase().contains("magic"));
}

#[test]
fn chunk_size_sane() {
    // 32x32 = 1024 chunks, each with one stone block at (0,0,0)
    // and otherwise air. Highly compressible; the total on-disk
    // footprint must stay well under 200 KB to confirm the format
    // pulls its weight on real-world dirty-chunk workloads.
    let tmp = TempDir::new().unwrap();
    let mut chunk = Chunk::empty();
    chunk.set(0, 0, 0, BlockId(1));
    let voxels = ChunkVoxels::from_chunk(&chunk);
    let mut total: u64 = 0;
    for cx in 0..32i32 {
        for cz in 0..32i32 {
            let payload = ChunkPayload {
                format_version: CHUNK_PAYLOAD_VERSION,
                coord: [cx, 0, cz],
                voxels: voxels.clone(),
                liquid_levels: Vec::new(),
                torch_orient: Vec::new(),
                snow_layers: Vec::new(),
            };
            write_chunk_in_dir(tmp.path(), "slot", "overworld", &payload).unwrap();
            let path =
                chunk_path_in_dir(tmp.path(), "slot", "overworld", [cx, 0, cz]);
            total += std::fs::metadata(&path).unwrap().len();
        }
    }
    assert!(
        total < 200 * 1024,
        "1024 sparse chunks should fit in < 200 KB; got {total}"
    );
}
