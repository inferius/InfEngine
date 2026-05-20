//! W29 tests: creative tab + mob skin + icon UV fixes.
//!
//! W29.6 test coverage for:
//!   - icon_uv_for fallback returns safe constant (not legacy 4x4 garbage).
//!   - Creative tab blacklist excludes fire (49), portal (47), end portal.
//!   - mob_skin::mob_skin_path resolves both .tga and .png.

use infengine_voxel::BlockId;

use crate::resource_pack::fallback::placeholder_pack;
use crate::creative_tab::{available_items, CREATIVE_BLACKLIST};

// ---------------------------------------------------------------------------
// W29.1 - icon_uv_for safe fallback
// ---------------------------------------------------------------------------

/// For an UNKNOWN block id (one with no name_map entry), icon_uv_for
/// must return the top-left tile of the atlas, not the legacy 4x4 rect.
/// On the 64x64 placeholder pack the top-left tile is [0,0, 0.25, 0.25].
#[test]
fn icon_uv_for_unknown_block_returns_safe_constant() {
    let pack = placeholder_pack();
    // Use a block id that has NO name_map entry so both icon_ref and
    // block_face_ref return None, forcing the W29.1 fallback path.
    // Id 9999 is well outside the registered range.
    let uv = pack.icon_uv_for(BlockId(9999), [0, 1, 0]);
    // On the 64x64 placeholder atlas, 16/64 = 0.25.
    let expected_u1 = 16.0_f32 / 64.0;
    let expected_v1 = 16.0_f32 / 64.0;
    assert_eq!(uv[0], 0.0, "u0 must be 0");
    assert_eq!(uv[1], 0.0, "v0 must be 0");
    assert!(
        (uv[2] - expected_u1).abs() < 1e-6,
        "u1 must be 16/atlas_w = {expected_u1}, got {}",
        uv[2]
    );
    assert!(
        (uv[3] - expected_v1).abs() < 1e-6,
        "v1 must be 16/atlas_h = {expected_v1}, got {}",
        uv[3]
    );
}

/// The fallback rect must be strictly less than 1.0 in both dimensions
/// so the caller cannot accidentally sample the entire atlas.
#[test]
fn icon_uv_for_fallback_does_not_span_full_atlas() {
    let pack = placeholder_pack();
    let uv = pack.icon_uv_for(BlockId(9999), [0, 1, 0]);
    assert!(uv[2] < 1.0, "u1 must be < 1 (not full atlas width), got {}", uv[2]);
    assert!(uv[3] < 1.0, "v1 must be < 1 (not full atlas height), got {}", uv[3]);
}

/// icon_uv_front must call icon_uv_for with [0,1,0] and return the same
/// result.
#[test]
fn icon_uv_front_consistent_with_icon_uv_for() {
    let pack = placeholder_pack();
    let via_for   = pack.icon_uv_for(BlockId(9999), [0, 1, 0]);
    let via_front = pack.icon_uv_front(BlockId(9999));
    assert_eq!(via_for, via_front, "icon_uv_front must match icon_uv_for([0,1,0])");
}

// ---------------------------------------------------------------------------
// W29.2 - Creative tab blacklist
// ---------------------------------------------------------------------------

/// fire (id 49) must NOT appear in available_items().
#[test]
fn creative_tab_excludes_fire() {
    let items = available_items();
    assert!(
        !items.iter().any(|e| e.id == 49),
        "fire (id 49) must be excluded from creative tab"
    );
}

/// portal (id 47) must NOT appear in available_items().
#[test]
fn creative_tab_excludes_portal() {
    let items = available_items();
    assert!(
        !items.iter().any(|e| e.id == 47),
        "portal (id 47) must be excluded from creative tab"
    );
}

/// soul_fire (id 56) must NOT appear in available_items().
#[test]
fn creative_tab_excludes_soul_fire() {
    let items = available_items();
    assert!(
        !items.iter().any(|e| e.id == 56),
        "soul_fire (id 56) must be excluded from creative tab"
    );
}

/// water source (id 36) must NOT appear in available_items().
#[test]
fn creative_tab_excludes_water_source() {
    let items = available_items();
    assert!(
        !items.iter().any(|e| e.id == 36),
        "water source (id 36) must be excluded from creative tab"
    );
}

/// lava source (id 37) must NOT appear in available_items().
#[test]
fn creative_tab_excludes_lava_source() {
    let items = available_items();
    assert!(
        !items.iter().any(|e| e.id == 37),
        "lava source (id 37) must be excluded from creative tab"
    );
}

/// Every blacklisted id must actually be absent from available_items().
#[test]
fn all_blacklisted_ids_absent_from_creative_tab() {
    let items = available_items();
    for &blacklisted in CREATIVE_BLACKLIST {
        assert!(
            !items.iter().any(|e| e.id == blacklisted),
            "blacklisted id {blacklisted} must not appear in creative tab"
        );
    }
}

/// Useful placeable blocks (stone=1, torch=10, chest=41) must still appear.
#[test]
fn creative_tab_retains_normal_blocks() {
    let items = available_items();
    for id in [1u16, 10, 41] {
        assert!(
            items.iter().any(|e| e.id == id),
            "normal block id {id} must still appear in creative tab"
        );
    }
}
