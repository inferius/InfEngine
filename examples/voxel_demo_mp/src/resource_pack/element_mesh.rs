//! Per-element geometry emission for MC block models (W16.2).
//!
//! The standard chunk mesher in `infengine_voxel` emits full 6-face
//! cubes. This module provides `emit_block_elements`, which is
//! supplied to `chunk_mesh_with_atlas_light_anim_and_special` as the
//! `emit_special` closure.
//!
//! For each element in a `BlockModel`:
//!   1. Scale `from`/`to` from MC 16ths -> world units (divide by 16).
//!   2. Build the 6 axis-aligned face quads of the element's AABB.
//!   3. Apply `ElementRotation` (if any) using `glam::Mat4`.
//!   4. Resolve the face texture via `ResourcePack::block_texture`.
//!   5. Emit 4 vertices + 6 indices via `infengine_voxel::emit_element_quad`.
//!
//! `cullface` handling: if a face declares a `CubeFace` cullface
//! direction AND the voxel's neighbour in that direction is opaque,
//! the face is skipped (matches MC's culling contract).
//!
//! Callers: see `build_lit_submeshes` in `main.rs` which captures
//! lighting + anim lookups and forwards them here.
//!
//! W44.1: debug logging gated by `RUST_LOG=voxel_demo_mp::resource_pack=debug`
//! OR by the runtime toggle `enable_mesher_debug_logging(true)`. The
//! toggle allows one-frame capture from a keypress (F4) without
//! restarting with env-var set.

use std::sync::atomic::{AtomicBool, Ordering};

use glam::{Mat4, Vec3, Vec4};
use infengine_render::MeshVertex;

use super::models::{CubeFace, ElementFaces, ElementRotation, ModelFace, RotationAxis};
use super::special_models::model_for_block_with_neighbors;
use super::ResourcePack;

// Re-export so callers can build NeighborStates without reaching into blockstates.
pub(crate) use super::blockstates::NeighborStates;

// ---------------------------------------------------------------------------
// W44.1: runtime debug logging toggle.
// ---------------------------------------------------------------------------

/// When `true`, emit per-face tracing::debug! lines inside
/// `emit_block_elements`. Toggle via `enable_mesher_debug_logging`.
/// The env-var gate (`RUST_LOG=...=debug`) is a second, independent
/// gate: logs only fire when BOTH are enabled.
static MESHER_DEBUG: AtomicBool = AtomicBool::new(false);

/// Enable or disable per-frame mesher debug logging at runtime.
///
/// Intended usage: bind to F4 in main.rs, enable for one frame, then
/// disable so the log spam doesn't continue every chunk remesh.
///
/// ```ignore
/// // In key-press handler:
/// resource_pack::element_mesh::enable_mesher_debug_logging(true);
/// // After one mesh rebuild:
/// resource_pack::element_mesh::enable_mesher_debug_logging(false);
/// ```
pub(crate) fn enable_mesher_debug_logging(on: bool) {
    MESHER_DEBUG.store(on, Ordering::Relaxed);
}

/// Attempt to emit model-element geometry for `block_id` at
/// `voxel_origin` (world-space corner of the voxel).
///
/// Returns `true` if the block has a registered model AND at least
/// one vertex was emitted. Returns `false` when no model exists OR
/// when all faces were culled / had unresolvable textures (caller
/// falls back to the cube path).
///
/// W43.1: honest return value - previously returned `true` even when
/// emit_faces_for_element pushed 0 vertices (e.g. all faces culled
/// because every neighbour is opaque, or texture name lookup miss).
/// The cube path is now the automatic fallback for any block where
/// the JSON pipeline silently drops all faces.
///
/// `neighbors` carries the horizontal connection flags (north/south/
/// east/west/up/down) used by multipart blockstates (fences, walls,
/// panes). Pass `NeighborStates::default()` when caller does not yet
/// provide this info - unconditional parts still render correctly.
/// `uv_for_face` is called for each element face to resolve the
/// atlas UV rect `[u0, v0, u1, v1]`.
/// `light_at_neighbour` is called with the axis-aligned neighbour
/// direction to sample the lighting value for that face.
/// `anim_id_for_face` is called to resolve the anim slot id.
/// `neighbour_opaque` is called with the voxel-local signed delta
/// `[dx, dy, dz]` to check whether that neighbour culls the face.
#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_block_elements(
    block_id: u16,
    voxel_origin: Vec3,
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    pack: &ResourcePack,
    neighbors: NeighborStates,
    light_at_neighbour: impl Fn([i8; 3]) -> [f32; 2],
    anim_id_for_face: impl Fn(&str) -> u32,
    neighbour_opaque: impl Fn([i8; 3]) -> bool,
) -> bool {
    // W40.1: use neighbor-aware lookup so multipart blockstates
    // (fences, walls, panes) show connection arms correctly.
    let Some(model) = model_for_block_with_neighbors(block_id, pack, &neighbors) else {
        return false;
    };

    // W43.1: record vertex count before emission. If no vertex is
    // added across all elements (all faces culled or texture miss),
    // return false so the caller's cube path handles the block.
    let verts_before = vertices.len();

    let dbg = MESHER_DEBUG.load(Ordering::Relaxed);

    for elem in &model.elements {
        // Scale from MC 16ths to world units: 1 voxel = 1.0 world unit.
        let from = Vec3::new(
            elem.from[0] / 16.0,
            elem.from[1] / 16.0,
            elem.from[2] / 16.0,
        );
        let to = Vec3::new(
            elem.to[0] / 16.0,
            elem.to[1] / 16.0,
            elem.to[2] / 16.0,
        );

        // Precompute the rotation matrix for this element (if any).
        let rot_mat = elem.rotation.map(build_rotation_mat);

        // W44.1: debug logging.
        if dbg {
            tracing::debug!(
                block_id,
                elem_from = ?elem.from,
                elem_to = ?elem.to,
                "emit_block_elements: processing element",
            );
        }

        // Emit each declared face.
        emit_faces_for_element(
            block_id,
            &elem.faces,
            from,
            to,
            voxel_origin,
            rot_mat,
            vertices,
            indices,
            pack,
            &light_at_neighbour,
            &anim_id_for_face,
            &neighbour_opaque,
            dbg,
        );
    }

    let faces_emitted = (vertices.len() - verts_before) / 4;
    if dbg {
        tracing::debug!(
            block_id,
            elements = model.elements.len(),
            faces_emitted,
            verts_total = vertices.len() - verts_before,
            "emit_block_elements result",
        );
    }

    // At least one vertex must have been added for the emit to count
    // as successful. Zero vertices -> all faces were culled or the
    // texture map had no resolvable entries -> fall through to cube.
    vertices.len() > verts_before
}

// ---------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------

/// Build a rotation `Mat4` from an `ElementRotation`. The matrix
/// rotates around `origin` (in 16ths, scaled to world units) by
/// `angle_deg` on `axis`. Rescale is noted but not implemented here
/// (it is only needed for the cross pattern's anti-gap expansion,
/// which is already baked into the from/to geometry in
/// `build_cross`).
fn build_rotation_mat(r: ElementRotation) -> Mat4 {
    let origin = Vec3::new(r.origin[0] / 16.0, r.origin[1] / 16.0, r.origin[2] / 16.0);
    let angle_rad = r.angle_deg.to_radians();
    let axis = match r.axis {
        RotationAxis::X => Vec3::X,
        RotationAxis::Y => Vec3::Y,
        RotationAxis::Z => Vec3::Z,
    };
    // Translate to origin, rotate, translate back.
    Mat4::from_translation(origin)
        * Mat4::from_axis_angle(axis, angle_rad)
        * Mat4::from_translation(-origin)
}

/// Apply an optional rotation matrix to a Vec3 position.
fn apply_rot(pos: Vec3, mat: Option<Mat4>) -> Vec3 {
    match mat {
        Some(m) => {
            let v4 = m * Vec4::new(pos.x, pos.y, pos.z, 1.0);
            Vec3::new(v4.x, v4.y, v4.z)
        }
        None => pos,
    }
}

/// One axis-aligned face of a box AABB. Defines the 4 corners
/// (in box-local space, 0-1 fraction) + the face normal direction.
struct BoxFace {
    /// Outward normal as integer signs. Matches the CubeFace variants.
    normal: [i8; 3],
    /// Cube face direction (for cullface matching).
    cube_face: CubeFace,
    /// Fraction offsets for the 4 corners relative to the box.
    /// Layout: (u_axis, v_axis) in CCW order viewed from normal.
    corners_fn: fn(from: Vec3, to: Vec3) -> [[f32; 3]; 4],
}

const BOX_FACES: &[BoxFace] = &[
    // +X (east)
    BoxFace {
        normal: [1, 0, 0],
        cube_face: CubeFace::East,
        corners_fn: |from, to| {
            [
                [to.x, from.y, from.z],
                [to.x, from.y, to.z],
                [to.x, to.y, to.z],
                [to.x, to.y, from.z],
            ]
        },
    },
    // -X (west)
    BoxFace {
        normal: [-1, 0, 0],
        cube_face: CubeFace::West,
        corners_fn: |from, to| {
            [
                [from.x, from.y, to.z],
                [from.x, from.y, from.z],
                [from.x, to.y, from.z],
                [from.x, to.y, to.z],
            ]
        },
    },
    // +Y (up)
    BoxFace {
        normal: [0, 1, 0],
        cube_face: CubeFace::Up,
        corners_fn: |from, to| {
            [
                [from.x, to.y, from.z],
                [to.x, to.y, from.z],
                [to.x, to.y, to.z],
                [from.x, to.y, to.z],
            ]
        },
    },
    // -Y (down)
    BoxFace {
        normal: [0, -1, 0],
        cube_face: CubeFace::Down,
        corners_fn: |from, to| {
            [
                [from.x, from.y, to.z],
                [to.x, from.y, to.z],
                [to.x, from.y, from.z],
                [from.x, from.y, from.z],
            ]
        },
    },
    // +Z (south)
    BoxFace {
        normal: [0, 0, 1],
        cube_face: CubeFace::South,
        corners_fn: |from, to| {
            [
                [to.x, from.y, to.z],
                [from.x, from.y, to.z],
                [from.x, to.y, to.z],
                [to.x, to.y, to.z],
            ]
        },
    },
    // -Z (north)
    BoxFace {
        normal: [0, 0, -1],
        cube_face: CubeFace::North,
        corners_fn: |from, to| {
            [
                [from.x, from.y, from.z],
                [to.x, from.y, from.z],
                [to.x, to.y, from.z],
                [from.x, to.y, from.z],
            ]
        },
    },
];

/// Emit the declared faces of a single `ModelElement` AABB.
#[allow(clippy::too_many_arguments)]
fn emit_faces_for_element(
    block_id: u16,
    faces: &ElementFaces,
    from: Vec3,
    to: Vec3,
    voxel_origin: Vec3,
    rot_mat: Option<Mat4>,
    vertices: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    pack: &ResourcePack,
    light_at_neighbour: &impl Fn([i8; 3]) -> [f32; 2],
    anim_id_for_face: &impl Fn(&str) -> u32,
    neighbour_opaque: &impl Fn([i8; 3]) -> bool,
    dbg: bool,
) {
    for box_face in BOX_FACES {
        // Pick the declared face for this direction.
        let maybe_face: Option<&ModelFace> = match box_face.cube_face {
            CubeFace::East => faces.east.as_ref(),
            CubeFace::West => faces.west.as_ref(),
            CubeFace::Up => faces.up.as_ref(),
            CubeFace::Down => faces.down.as_ref(),
            CubeFace::South => faces.south.as_ref(),
            CubeFace::North => faces.north.as_ref(),
        };
        let Some(model_face) = maybe_face else {
            continue; // face not declared in model
        };

        // Cullface: if the model says to cull when the axis-aligned
        // neighbour in `cullface`'s direction is opaque, skip.
        let culled = if let Some(cf) = model_face.cullface {
            let delta = cube_face_to_normal(cf);
            let opaque = neighbour_opaque(delta);
            if dbg {
                tracing::debug!(
                    block_id,
                    box_face = ?box_face.cube_face,
                    cullface = ?cf,
                    delta = ?delta,
                    neighbour_opaque = opaque,
                    "face evaluation",
                );
            }
            opaque
        } else {
            if dbg {
                tracing::debug!(
                    block_id,
                    box_face = ?box_face.cube_face,
                    cullface = "none",
                    "face evaluation (no cullface - always emit)",
                );
            }
            false
        };
        if culled {
            continue;
        }

        // Resolve texture -> atlas UV.
        let tex_name = &model_face.texture;
        // Strip a leading "#" (unresolved var) - fall back to the
        // bare stem, or use the block's face entry if the name is
        // a variable that was never resolved.
        // W43.3: also strip any residual "block/" / "item/" prefix
        // that may survive from hand-coded models or an edge case in
        // the resolution chain (models::resolve_texture already does
        // this via rsplit_once, but be defensive here too).
        let after_hash = tex_name.trim_start_matches('#');
        let clean_name = after_hash
            .rsplit_once('/')
            .map(|(_, stem)| stem)
            .unwrap_or(after_hash);
        // W37 fix: fall back to the magenta __missing sentinel when
        // the texture name isn't registered. The previous
        // [0.0, 0.0, 1.0, 1.0] fallback covered the WHOLE atlas as
        // one face's UV which made torches / special-model blocks
        // render as a chaotic mosaic of every atlas tile (user's
        // "mix spritu" report). The missing sentinel makes such
        // blocks visibly magenta-checkered so the user knows the
        // texture key didn't resolve.
        let atlas_uv = pack
            .block_texture(clean_name)
            .map(|t| t.atlas_uv)
            .or_else(|| {
                pack.block_texture("__missing").map(|t| t.atlas_uv)
            })
            .unwrap_or([0.0, 0.0, 1.0, 1.0]);

        // Scale UV from MC 16ths to 0..1.
        let uv_rect = scale_uv(model_face.uv, atlas_uv);

        // Compute the 4 corners in element-local space (scaled to
        // world units), then apply rotation + translate by voxel
        // origin.
        let raw = (box_face.corners_fn)(from, to);
        let mut world_corners = [[0.0f32; 3]; 4];
        for (i, &[cx, cy, cz]) in raw.iter().enumerate() {
            let local = Vec3::new(cx, cy, cz);
            let rotated = apply_rot(local, rot_mat);
            let world = voxel_origin + rotated;
            world_corners[i] = [world.x, world.y, world.z];
        }

        // Compute a rotated normal for lighting sample direction.
        let base_normal = Vec3::new(
            box_face.normal[0] as f32,
            box_face.normal[1] as f32,
            box_face.normal[2] as f32,
        );
        let rot_normal = match rot_mat {
            Some(m) => {
                let n4 = m * Vec4::new(base_normal.x, base_normal.y, base_normal.z, 0.0);
                Vec3::new(n4.x, n4.y, n4.z).normalize_or_zero()
            }
            None => base_normal,
        };
        // Use the face's axis-aligned direction for the light lookup
        // (lighting is sampled per-voxel, so we snap back to the
        // nearest cardinal direction for the air-side cell offset).
        let light_normal = if rot_mat.is_some() {
            // Snap the rotated normal to the closest cardinal.
            snap_to_cardinal(rot_normal)
        } else {
            box_face.normal
        };
        let light = light_at_neighbour(light_normal);
        let anim_id = anim_id_for_face(clean_name);

        let emit_normal = [rot_normal.x, rot_normal.y, rot_normal.z];
        infengine_voxel::emit_element_quad(
            vertices,
            indices,
            world_corners,
            emit_normal,
            uv_rect,
            light,
            anim_id,
        );

        // W46.29: planar elements (one axis has from == to) declare
        // only ONE face direction in the JSON, so the matching
        // BOX_FACES side-pass emits a single quad whose back side
        // gets culled by the front_face = Ccw pipeline state. Result:
        // the plane is invisible from the OTHER half-space. This is
        // exactly the case for redstone_torch's 6 halo planes around
        // the lit tip - hence the user reported "redstone has no red
        // glow around the head".
        //
        // Emit a second quad with reversed corner order so the plane
        // is visible from both sides. Only triggered when the source
        // element is degenerate on one axis (no extra geometry for
        // normal cube blocks).
        let is_planar =
            from.x == to.x || from.y == to.y || from.z == to.z;
        if is_planar {
            let mirrored: [[f32; 3]; 4] = [
                world_corners[0],
                world_corners[3],
                world_corners[2],
                world_corners[1],
            ];
            let mirrored_normal = [-emit_normal[0], -emit_normal[1], -emit_normal[2]];
            infengine_voxel::emit_element_quad(
                vertices,
                indices,
                mirrored,
                mirrored_normal,
                uv_rect,
                light,
                anim_id,
            );
        }
    }
}

/// Map a `CubeFace` enum variant to an integer normal delta.
fn cube_face_to_normal(face: CubeFace) -> [i8; 3] {
    match face {
        CubeFace::East => [1, 0, 0],
        CubeFace::West => [-1, 0, 0],
        CubeFace::Up => [0, 1, 0],
        CubeFace::Down => [0, -1, 0],
        CubeFace::South => [0, 0, 1],
        CubeFace::North => [0, 0, -1],
    }
}

/// Snap a Vec3 direction to the closest axis-aligned cardinal normal.
fn snap_to_cardinal(n: Vec3) -> [i8; 3] {
    let ax = n.x.abs();
    let ay = n.y.abs();
    let az = n.z.abs();
    if ax >= ay && ax >= az {
        if n.x >= 0.0 { [1, 0, 0] } else { [-1, 0, 0] }
    } else if ay >= ax && ay >= az {
        if n.y >= 0.0 { [0, 1, 0] } else { [0, -1, 0] }
    } else if n.z >= 0.0 {
        [0, 0, 1]
    } else {
        [0, 0, -1]
    }
}

/// Scale a face UV from MC 16ths-per-tile coordinates to the atlas
/// sub-rect occupied by this tile.
///
/// `model_uv` is `[u0, v0, u1, v1]` in 0..16 tile space.
/// `atlas_uv` is `[au0, av0, au1, av1]` in 0..1 atlas space.
/// Returns the remapped UV in atlas space.
fn scale_uv(model_uv: [f32; 4], atlas_uv: [f32; 4]) -> [f32; 4] {
    let tile_w = atlas_uv[2] - atlas_uv[0];
    let tile_h = atlas_uv[3] - atlas_uv[1];
    [
        atlas_uv[0] + (model_uv[0] / 16.0) * tile_w,
        atlas_uv[1] + (model_uv[1] / 16.0) * tile_h,
        atlas_uv[0] + (model_uv[2] / 16.0) * tile_w,
        atlas_uv[1] + (model_uv[3] / 16.0) * tile_h,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snap_to_cardinal_returns_closest_axis() {
        // Mostly-Y vector snaps to +Y.
        assert_eq!(snap_to_cardinal(Vec3::new(0.1, 0.9, 0.1)), [0, 1, 0]);
        // Mostly-X negative snaps to -X.
        assert_eq!(snap_to_cardinal(Vec3::new(-0.8, 0.1, 0.2)), [-1, 0, 0]);
        // Pure Z snaps to +Z.
        assert_eq!(snap_to_cardinal(Vec3::Z), [0, 0, 1]);
        // Negative Z snaps to -Z.
        assert_eq!(snap_to_cardinal(-Vec3::Z), [0, 0, -1]);
    }

    #[test]
    fn scale_uv_full_tile_identity() {
        // Full 0..16 model UV on a full 0..1 atlas tile is unchanged.
        let result = scale_uv([0.0, 0.0, 16.0, 16.0], [0.0, 0.0, 1.0, 1.0]);
        let want = [0.0_f32, 0.0, 1.0, 1.0];
        for (r, w) in result.iter().zip(want.iter()) {
            assert!((r - w).abs() < 1e-5, "scale_uv mismatch: {r} vs {w}");
        }
    }

    #[test]
    fn scale_uv_half_tile() {
        // Lower half of tile (v 8..16) on an atlas tile at y 0.5..0.75.
        let result = scale_uv([0.0, 8.0, 16.0, 16.0], [0.0, 0.5, 1.0, 0.75]);
        // v0 = 0.5 + (8/16)*0.25 = 0.5 + 0.125 = 0.625
        // v1 = 0.5 + (16/16)*0.25 = 0.75
        assert!((result[1] - 0.625).abs() < 1e-5, "v0 mismatch");
        assert!((result[3] - 0.75).abs() < 1e-5, "v1 mismatch");
    }

    #[test]
    fn build_rotation_mat_y_90_rotates_x_to_neg_z() {
        // glam right-hand rule: +90 deg around Y maps +X -> -Z.
        let rot = ElementRotation {
            origin: [0.0, 0.0, 0.0],
            axis: RotationAxis::Y,
            angle_deg: 90.0,
            rescale: false,
        };
        let mat = build_rotation_mat(rot);
        let v4 = mat * Vec4::new(1.0, 0.0, 0.0, 1.0);
        // +X rotated +90 deg around Y gives -Z in right-hand convention.
        assert!((v4.z + 1.0).abs() < 1e-4, "expected z~-1, got {}", v4.z);
        assert!(v4.x.abs() < 1e-4, "expected x~0, got {}", v4.x);
    }

    #[test]
    fn cube_face_to_normal_all_six() {
        assert_eq!(cube_face_to_normal(CubeFace::East), [1, 0, 0]);
        assert_eq!(cube_face_to_normal(CubeFace::West), [-1, 0, 0]);
        assert_eq!(cube_face_to_normal(CubeFace::Up), [0, 1, 0]);
        assert_eq!(cube_face_to_normal(CubeFace::Down), [0, -1, 0]);
        assert_eq!(cube_face_to_normal(CubeFace::South), [0, 0, 1]);
        assert_eq!(cube_face_to_normal(CubeFace::North), [0, 0, -1]);
    }

    fn make_pack() -> super::super::ResourcePack {
        super::super::fallback::placeholder_pack()
    }

    #[test]
    fn emit_block_elements_returns_false_for_unknown_block() {
        // Block id 1 (stone) has no special model.
        // We pass no-op closures to prove it returns false cleanly.
        let pack = make_pack();
        let mut verts: Vec<MeshVertex> = Vec::new();
        let mut idxs: Vec<u32> = Vec::new();
        let handled = emit_block_elements(
            1,
            Vec3::ZERO,
            &mut verts,
            &mut idxs,
            &pack,
            NeighborStates::default(),
            |_| [0.0, 1.0],
            |_| 0,
            |_| false,
        );
        assert!(!handled, "stone should not be handled");
        assert!(verts.is_empty());
        assert!(idxs.is_empty());
    }

    #[test]
    fn emit_block_elements_flower_emits_nonzero_geometry() {
        use crate::block::FLOWER_RED_ID;
        // Flower (id 20) has a cross model with 4 faces (2 quads
        // * 2 sides each). Each quad emits 4 vertices + 6 indices.
        let pack = make_pack();
        let mut verts: Vec<MeshVertex> = Vec::new();
        let mut idxs: Vec<u32> = Vec::new();
        let handled = emit_block_elements(
            FLOWER_RED_ID,
            Vec3::ZERO,
            &mut verts,
            &mut idxs,
            &pack,
            NeighborStates::default(),
            |_| [0.0, 1.0],
            |_| 0,
            |_| false, // no neighbours opaque
        );
        assert!(handled, "flower should be handled by special model path");
        // 4 faces * 4 verts = 16 vertices, 4 faces * 6 indices = 24.
        assert_eq!(
            verts.len(),
            16,
            "expected 16 vertices (4 faces), got {}",
            verts.len(),
        );
        assert_eq!(
            idxs.len(),
            24,
            "expected 24 indices (4 faces * 6), got {}",
            idxs.len(),
        );
    }

    #[test]
    fn emit_block_elements_torch_returns_true() {
        use crate::block::TORCH_ID;
        let pack = make_pack();
        let mut verts: Vec<MeshVertex> = Vec::new();
        let mut idxs: Vec<u32> = Vec::new();
        let handled = emit_block_elements(
            TORCH_ID,
            Vec3::ZERO,
            &mut verts,
            &mut idxs,
            &pack,
            NeighborStates::default(),
            |_| [0.0, 1.0],
            |_| 0,
            |_| false,
        );
        assert!(handled, "torch should be handled");
        assert!(!verts.is_empty(), "torch should emit some vertices");
    }

    #[test]
    fn emit_block_elements_slab_emits_top_face_at_y_0_5() {
        use crate::block::OAK_SLAB_ID;
        let pack = make_pack();
        let mut verts: Vec<MeshVertex> = Vec::new();
        let mut idxs: Vec<u32> = Vec::new();
        let origin = Vec3::new(0.0, 0.0, 0.0);
        let handled = emit_block_elements(
            OAK_SLAB_ID,
            origin,
            &mut verts,
            &mut idxs,
            &pack,
            NeighborStates::default(),
            |_| [0.0, 1.0],
            |_| 0,
            |_| false,
        );
        assert!(handled, "slab should be handled");
        // Check max Y in emitted vertices is 0.5 (not 1.0).
        let max_y = verts
            .iter()
            .map(|v| v.position[1])
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            (max_y - 0.5).abs() < 1e-4,
            "slab top face should be at y=0.5, got y={max_y}",
        );
    }

    // ---------------------------------------------------------------
    // W43.5: honest return value tests.
    // ---------------------------------------------------------------

    /// W43.1: emit returns false when every face of the model is
    /// culled by an always-opaque neighbour.
    ///
    /// The torch block is a known special model where the DOWN face
    /// has `cullface: Down`. With neighbour_opaque always true the
    /// down face is culled; the remaining 5 side/up faces have NO
    /// cullface so they still emit. This test therefore verifies the
    /// positive path: torch returns true even when down is culled.
    ///
    /// For the "returns false" case we use a synthetic cube model via
    /// the pipeline_tests path (see pipeline_tests.rs::W43 section).
    #[test]
    fn emit_block_elements_torch_still_emits_with_opaque_down_neighbour() {
        use crate::block::TORCH_ID;
        let pack = make_pack();
        let mut verts: Vec<MeshVertex> = Vec::new();
        let mut idxs: Vec<u32> = Vec::new();
        let handled = emit_block_elements(
            TORCH_ID,
            Vec3::ZERO,
            &mut verts,
            &mut idxs,
            &pack,
            NeighborStates::default(),
            |_| [0.0, 1.0],
            |_| 0,
            // All neighbours opaque - only the cullface=Down face is
            // culled; the 5 side + up faces have no cullface and emit.
            |_| true,
        );
        assert!(
            handled,
            "torch must return true: 5 non-culled faces still emit",
        );
        // 5 non-culled faces (up + 4 sides, each 4 verts) = 20 verts.
        assert_eq!(
            verts.len(),
            20,
            "expected 20 verts (5 faces), got {}",
            verts.len(),
        );
    }

    /// W43.1: when zero vertices are emitted (all faces culled for
    /// blocks where ALL faces carry a cullface), emit_block_elements
    /// MUST return false so the caller's cube path runs.
    ///
    /// The slab is the easiest special model to test here: its down/
    /// north/south/west/east faces all carry a cullface; ONLY the up
    /// face does NOT. With neighbour_opaque=true for every direction
    /// those 5 faces are culled; the up face still emits 4 verts.
    /// Result: true (at least 1 face emitted).
    ///
    /// The full "0 verts -> false" path is covered in
    /// pipeline_tests.rs where we can build a synthetic cube model
    /// with all-cullface faces and pass neighbour_opaque=|_|true.
    #[test]
    fn emit_block_elements_slab_still_emits_up_face_when_sides_culled() {
        use crate::block::OAK_SLAB_ID;
        let pack = make_pack();
        let mut verts: Vec<MeshVertex> = Vec::new();
        let mut idxs: Vec<u32> = Vec::new();
        let handled = emit_block_elements(
            OAK_SLAB_ID,
            Vec3::ZERO,
            &mut verts,
            &mut idxs,
            &pack,
            NeighborStates::default(),
            |_| [0.0, 1.0],
            |_| 0,
            // All neighbours opaque: down+north+south+west+east culled;
            // up face (no cullface) still emits.
            |_| true,
        );
        assert!(
            handled,
            "slab with all-opaque neighbours must still return true (up face not culled)",
        );
        // Only the up face emits: 1 face * 4 verts = 4 verts.
        assert_eq!(
            verts.len(),
            4,
            "expected 4 verts (up face only, rest culled), got {}",
            verts.len(),
        );
    }
}
