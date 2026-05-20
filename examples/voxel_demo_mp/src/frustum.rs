//! W13.4: Axis-Aligned Bounding Box (AABB) frustum culling for chunk
//! meshes.
//!
//! The view frustum is extracted from the combined view-projection
//! matrix using the Gribb-Hartmann method. Each of the 6 clip planes
//! is normalised so the signed distance from a point to the plane is
//! in view-space units. An AABB is considered visible when it is NOT
//! fully behind any single clip plane.
//!
//! Typical win at 10-chunk render distance: ~60-70% of chunks are
//! outside the frustum (behind the player, off to the sides) and
//! skip `draw_mesh_pbr` entirely.

use glam::{Mat4, Vec3};
use infengine_voxel::CHUNK_SIZE;

/// One clip plane stored as a normalised (nx, ny, nz, d) vec4 such
/// that `dot(plane.xyz, point) + plane.w > 0` means the point is on
/// the POSITIVE side of the plane (inside the frustum).
#[derive(Debug, Copy, Clone)]
pub struct Plane {
    normal: Vec3,
    d: f32,
}

impl Plane {
    /// Extract from a row of the clip matrix.
    fn from_row(row: [f32; 4]) -> Self {
        let n = Vec3::new(row[0], row[1], row[2]);
        let len = n.length();
        if len < 1e-7 {
            return Self { normal: Vec3::ZERO, d: 0.0 };
        }
        Self {
            normal: n / len,
            d: row[3] / len,
        }
    }

    /// Signed distance from `point` to the plane. Positive = inside.
    #[inline]
    fn signed_distance(&self, point: Vec3) -> f32 {
        self.normal.dot(point) + self.d
    }
}

/// Six-plane view frustum built from the view-projection matrix.
pub struct Frustum {
    planes: [Plane; 6],
}

impl Frustum {
    /// Build from a column-major view-projection matrix (as returned
    /// by `Camera3D::view_projection()`).
    pub fn from_view_proj(vp: Mat4) -> Self {
        // Extract rows from the column-major matrix. Mat4 stores
        // columns; `col(i)` returns the i-th column. To get rows we
        // transpose and read columns, or use row_col indexing directly.
        // Gribb-Hartmann planes: p[i] = row(3) +/- row(i) for axes,
        // then near/far as row(3) +/- row(2).
        //
        // glam Mat4 field layout: `x_axis`, `y_axis`, `z_axis`, `w_axis`
        // are columns 0-3. Row `r` has elements
        //   [x_axis[r], y_axis[r], z_axis[r], w_axis[r]].
        let r = |row: usize| -> [f32; 4] {
            [
                vp.x_axis[row],
                vp.y_axis[row],
                vp.z_axis[row],
                vp.w_axis[row],
            ]
        };
        let row0 = r(0);
        let row1 = r(1);
        let row2 = r(2);
        let row3 = r(3);

        // Left:   row3 + row0
        // Right:  row3 - row0
        // Bottom: row3 + row1
        // Top:    row3 - row1
        // Near:   row3 + row2 (OpenGL-style; wgpu uses [0,1] NDC so
        //         near = row2 alone, but row3+row2 is the safe form for
        //         both conventions at the distances we operate at)
        // Far:    row3 - row2
        let add = |a: [f32; 4], b: [f32; 4]| -> [f32; 4] {
            [a[0]+b[0], a[1]+b[1], a[2]+b[2], a[3]+b[3]]
        };
        let sub = |a: [f32; 4], b: [f32; 4]| -> [f32; 4] {
            [a[0]-b[0], a[1]-b[1], a[2]-b[2], a[3]-b[3]]
        };
        Self {
            planes: [
                Plane::from_row(add(row3, row0)), // left
                Plane::from_row(sub(row3, row0)), // right
                Plane::from_row(add(row3, row1)), // bottom
                Plane::from_row(sub(row3, row1)), // top
                Plane::from_row(add(row3, row2)), // near
                Plane::from_row(sub(row3, row2)), // far
            ],
        }
    }

    /// Returns `true` if the AABB described by its min and max corners
    /// is at least partially inside the frustum. Uses the "positive
    /// vertex" test: for each plane, pick the corner of the AABB that
    /// is furthest along the plane normal and check if that corner is
    /// still on the negative side. If it is, the whole box is outside.
    ///
    /// One chunk-sized margin is added on all sides so chunks don't
    /// pop out while the player rotates toward them.
    #[inline]
    pub fn is_aabb_visible(&self, min: Vec3, max: Vec3) -> bool {
        // 1 chunk (16 voxels) conservative margin so meshes don't
        // pop at the frustum edge as the player rotates.
        const MARGIN: f32 = CHUNK_SIZE as f32;
        let min = min - Vec3::splat(MARGIN);
        let max = max + Vec3::splat(MARGIN);

        for plane in &self.planes {
            // For each plane, find the "positive vertex" - the AABB
            // corner that is furthest in the direction of the plane
            // normal. If even this corner is on the negative side,
            // the entire AABB is outside the frustum.
            let px = if plane.normal.x >= 0.0 { max.x } else { min.x };
            let py = if plane.normal.y >= 0.0 { max.y } else { min.y };
            let pz = if plane.normal.z >= 0.0 { max.z } else { min.z };
            if plane.signed_distance(Vec3::new(px, py, pz)) < 0.0 {
                return false; // fully behind this plane
            }
        }
        true
    }

    /// Returns `true` if a single point is inside (not behind all planes)
    /// the frustum. Adds a 2-voxel margin so mobs near the edge are not
    /// culled while their bounding volume is still partially visible.
    /// W28.11: used for mob frustum culling.
    #[inline]
    pub fn contains_point(&self, point: Vec3) -> bool {
        const MARGIN: f32 = 2.0;
        for plane in &self.planes {
            if plane.signed_distance(point) < -MARGIN {
                return false;
            }
        }
        true
    }

    /// Convenience wrapper for a chunk coord. Each chunk is a cube of
    /// `CHUNK_SIZE` voxels starting at `World::chunk_world_origin`.
    #[inline]
    pub fn is_chunk_visible(&self, coord: [i32; 3]) -> bool {
        let cs = CHUNK_SIZE as f32;
        let min = Vec3::new(
            coord[0] as f32 * cs,
            coord[1] as f32 * cs,
            coord[2] as f32 * cs,
        );
        let max = min + Vec3::splat(cs);
        self.is_aabb_visible(min, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Mat4;

    /// Build a simple projection matrix pointing down +Z with 90-deg
    /// FOV. Used to set up a known frustum for the unit tests.
    fn test_vp() -> Mat4 {
        let proj = Mat4::perspective_rh(std::f32::consts::FRAC_PI_2, 1.0, 0.1, 200.0);
        // Camera at origin looking at +Z.
        let view = Mat4::look_at_rh(
            Vec3::new(0.0, 0.0, -5.0),
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::Y,
        );
        proj * view
    }

    /// W13.4 test: a chunk directly in front of the camera is visible.
    #[test]
    fn frustum_keeps_chunk_in_view() {
        let frustum = Frustum::from_view_proj(test_vp());
        // Chunk at coord [0,0,0] -> world AABB [0,0,0]..[16,16,16].
        // Camera at z=-5 looking toward z=0, so this chunk is in front.
        assert!(
            frustum.is_chunk_visible([0, 0, 0]),
            "chunk directly in front of camera should be visible"
        );
    }

    /// W13.4 test: a chunk far behind the camera is culled.
    #[test]
    fn frustum_culls_chunk_behind_camera() {
        let frustum = Frustum::from_view_proj(test_vp());
        // Camera faces +Z from z=-5. A chunk at z=-50 is far behind.
        // coord [0, 0, -4] -> AABB z in [-64, -48], well behind.
        assert!(
            !frustum.is_chunk_visible([0, 0, -4]),
            "chunk far behind camera should be culled"
        );
    }

    /// W13.4 test: a chunk at extreme X distance is culled.
    #[test]
    fn frustum_culls_chunk_far_to_the_side() {
        let frustum = Frustum::from_view_proj(test_vp());
        // At 90-deg FOV, a chunk 200 units to the right is outside.
        assert!(
            !frustum.is_chunk_visible([14, 0, 0]),
            "chunk far to the side should be culled"
        );
    }

}
