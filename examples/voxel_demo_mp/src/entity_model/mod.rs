//! W91 - universal MC entity-model system.
//!
//! Mobs are described as **MC `ModelPart` data** - exactly the boxes,
//! pivots, rotations and `texOffs` from MC's Java models (`CowModel`,
//! `SheepModel`, `QuadrupedModel`, ...) - and turned into engine meshes
//! by ONE shared cube->mesh + MC->engine transform. No per-mob
//! eyeballed `Mat4::from_scale_rotation_translation` magic numbers (the
//! old approach scattered ~150 of those across `render/mobs.rs` and is
//! what put the cow's udder on its chest and dropped its horns).
//!
//! ## Coordinate systems
//!
//! MC entity models live in "model pixels", **Y-down**, and MC's
//! `LivingEntityRenderer` renders them with `poseStack.scale(-1,-1,1)`
//! then a 1/16 scale. We replicate that exactly: model->world negates X
//! and Y and scales by 1/16 (`MODEL_SCALE`). Determinant is +1 so face
//! winding is preserved. A per-model `ground_offset` (derived from the
//! lowest cube, i.e. the feet) lifts the model so the feet sit at the
//! entity's `pos.y`.
//!
//! ## Hierarchy
//!
//! A model is a flat list of `Bone`s (head / body / legs). Each bone
//! bakes all its cubes into one mesh in **bone-local** MC space (cube
//! coords minus the bone pivot), so the render only has to position the
//! bone via `pivot + rotation` (+ optional per-frame animation pose).
//! MC's models are shallow (one level under root) for these mobs, so a
//! flat list is faithful; nested children can be added later if a mob
//! needs them.

use glam::{Mat4, Quat, Vec3};
use infengine_render::{MeshHandle, MeshVertex, Renderer};

pub(crate) mod chest;
pub(crate) mod chicken;
pub(crate) mod cow;
pub(crate) mod minecart;
pub(crate) mod pig;
pub(crate) mod sheep;
pub(crate) mod skeleton;
pub(crate) mod spider;
pub(crate) mod zombie;

/// MC model pixels -> world blocks, with MC's X/Y flip baked in
/// (`scale(-1,-1,1)` * 1/16). Negating two axes keeps the determinant
/// positive, so triangle winding (and thus back-face culling) is
/// unaffected.
const MODEL_SCALE: f32 = 1.0 / 16.0;

/// One cuboid, matching MC `CubeListBuilder.addBox(..)` + `texOffs`.
/// All values are MC model pixels / pixel-UVs, straight from the Java
/// source - never pre-converted.
#[derive(Clone, Copy)]
pub(crate) struct McCube {
    /// `addBox` origin (min corner), MC pixels.
    pub origin: [f32; 3],
    /// `addBox` size (sx, sy, sz), MC pixels.
    pub size: [f32; 3],
    /// `texOffs(u, v)` - top-left of this cube's unwrap in the texture.
    pub uv: [u32; 2],
    /// `CubeDeformation` grow, MC pixels (usually 0).
    pub inflate: f32,
    /// `CubeListBuilder.mirror()` - flips the U direction of the unwrap.
    pub mirror: bool,
}

impl McCube {
    pub(crate) const fn new(origin: [f32; 3], size: [f32; 3], uv: [u32; 2]) -> Self {
        Self { origin, size, uv, inflate: 0.0, mirror: false }
    }
    /// MC `CubeListBuilder.mirror()` - flips the tile horizontally. Used by
    /// mobs whose left limbs reuse a right-limb texture (e.g. the spider's
    /// legs); quadrupeds share one non-mirrored leg builder so they don't.
    #[allow(dead_code)] // wired in as the biped/spider ports land (W92).
    pub(crate) const fn mirrored(mut self) -> Self {
        self.mirror = true;
        self
    }
    /// MC `CubeDeformation` grow (inflate) - e.g. hats, the sheep wool
    /// overlay. Builder form so model data stays declarative.
    #[allow(dead_code)] // wired in with the sheep/biped-hat ports (W92).
    pub(crate) const fn inflated(mut self, inflate: f32) -> Self {
        self.inflate = inflate;
        self
    }
}

/// One bone, matching MC `ModelPart` + `PartPose`.
#[derive(Clone, Copy)]
pub(crate) struct Bone {
    pub name: &'static str,
    /// `PartPose.offset` - the bone's pivot in the parent's MC space.
    pub pivot: [f32; 3],
    /// Default `PartPose` rotation (xRot, yRot, zRot) in radians.
    pub rotation: [f32; 3],
    pub cubes: &'static [McCube],
}

/// A full entity model: just its bones. The `texOffs` UVs are absolute
/// MC pixels and are normalized at bake time by the **actual loaded
/// texture's** dimensions, so one model definition works whatever size
/// MC ships the entity PNG at (e.g. the cow is 64x32 in classic packs,
/// 64x64 in modern ones - same `texOffs`, different normalization).
pub(crate) struct EntityModel {
    pub bones: &'static [Bone],
}

/// A bone's baked mesh handle + the source bone (for its transform).
pub(crate) struct BakedBone {
    pub bone: Bone,
    pub mesh: MeshHandle,
}

/// All baked bones for one entity, ready to draw.
pub(crate) struct BakedModel {
    pub bones: Vec<BakedBone>,
    /// World-space lift so the lowest cube (feet) sits at `pos.y`.
    pub ground_offset: f32,
}

/// Per-frame animation pose for a bone, layered on top of its default
/// `PartPose`. Empty = static. (Wired per-mob by the render layer:
/// head look, leg swing, etc.)
#[derive(Clone, Copy, Default)]
pub(crate) struct BonePose {
    pub extra_rotation: [f32; 3],
}

/// Build the engine meshes for an entity model and bind `img` as the
/// shared albedo for every bone. Call once at load. The albedo is made
/// once and `Arc`-cloned onto each bone mesh (no per-bone GPU upload).
pub(crate) fn bake(r: &mut Renderer, model: &EntityModel, img: &image::RgbaImage) -> BakedModel {
    // W102: bind the texture WITH its alpha intact. MC entity sheets have
    // genuinely transparent texels - e.g. the gaps between a skeleton's ribs
    // (you see through them), the chicken-leg and zombie-limb cutouts - and
    // the mesh-PBR shader discards alpha < 0.5, so those holes show the
    // background. W97 wrongly flattened alpha to 255 (copying the legacy
    // .tga-soft-edge workaround), which DISABLED the discard and turned
    // every transparent texel into opaque BLACK (the black skeleton ribs).
    // MC's .png sheets are hard-edged (alpha 0/255), so keeping alpha is
    // correct and matches how MC renders entities (cutout).
    let albedo = r.make_mesh_albedo(img.width(), img.height(), img);
    // UVs are normalized by the ACTUAL texture size (MC-asset-driven), not
    // a hardcoded model constant - so the same `texOffs` data is correct
    // whatever resolution the pack ships.
    let mut bones = Vec::with_capacity(model.bones.len());
    let mut lowest_world_y = 0.0_f32;
    for bone in model.bones {
        let (verts, indices) = bake_bone(bone, img);
        let mesh = r.create_mesh(bone.name, &verts, &indices);
        r.bind_mesh_albedo_arc(mesh, albedo.clone());
        bones.push(BakedBone { bone: *bone, mesh });
        // Track the lowest world point (the feet) to derive the ground
        // offset. We rotate each cube's 8 corners by the bone's default
        // rotation + pivot first, so splayed limbs (e.g. the spider's
        // legs, rotated ~45 deg down) are measured at their real tips, not
        // their un-rotated extents. world Y = -mc_y * scale, so the largest
        // MC y is the lowest world y.
        let rot = Quat::from_euler(
            glam::EulerRot::ZYX,
            bone.rotation[2],
            bone.rotation[1],
            bone.rotation[0],
        );
        let pivot = Vec3::from(bone.pivot);
        for c in bone.cubes {
            let [ox, oy, oz] = c.origin;
            let [sx, sy, sz] = c.size;
            let g = c.inflate;
            for &cx in &[ox - g, ox + sx + g] {
                for &cy in &[oy - g, oy + sy + g] {
                    for &cz in &[oz - g, oz + sz + g] {
                        let p = rot * Vec3::new(cx, cy, cz) + pivot;
                        lowest_world_y = lowest_world_y.min(-p.y * MODEL_SCALE);
                    }
                }
            }
        }
    }
    BakedModel { bones, ground_offset: -lowest_world_y }
}

/// Bake one bone's cubes into a single mesh, expressed in bone-local MC
/// pixels (cube coords as-is; the bone pivot is applied at draw time).
fn bake_bone(bone: &Bone, img: &image::RgbaImage) -> (Vec<MeshVertex>, Vec<u32>) {
    let mut verts: Vec<MeshVertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for cube in bone.cubes {
        append_cube(&mut verts, &mut indices, cube, img);
    }
    (verts, indices)
}

/// Alpha coverage of one face's UV tile, sampled from the entity sheet
/// at bake time (threshold mirrors the shader's `alpha < 0.5` discard).
#[derive(PartialEq, Clone, Copy, Debug)]
enum FaceCover {
    /// Every texel opaque - bake the face single-sided (backface culling
    /// hides the interior exactly like a closed MC box).
    Opaque,
    /// Mix of opaque + transparent texels - bake BOTH windings. MC draws
    /// entities with `RenderType.entityCutoutNoCull`, so a mostly-
    /// transparent face (the classic chicken leg) must show its art from
    /// either side; our mesh pipeline backface-culls, hence the second
    /// reversed copy (same depth, so exactly one side wins per view).
    Mixed,
    /// Every texel transparent - the face would discard entirely; skip
    /// emitting it at all.
    Empty,
}

/// Classify the pixel rect `[u_lo..u_hi) x [v_lo..v_hi)` of the sheet.
/// Degenerate rects (zero-size cubes' edge faces, out-of-sheet offsets)
/// count as `Empty`.
fn face_cover(img: &image::RgbaImage, u_lo: f32, u_hi: f32, v_lo: f32, v_hi: f32) -> FaceCover {
    let x0 = u_lo.min(u_hi).floor().max(0.0) as u32;
    let x1 = (u_lo.max(u_hi).ceil().max(0.0) as u32).min(img.width());
    let y0 = v_lo.min(v_hi).floor().max(0.0) as u32;
    let y1 = (v_lo.max(v_hi).ceil().max(0.0) as u32).min(img.height());
    if x0 >= x1 || y0 >= y1 {
        return FaceCover::Empty;
    }
    let mut any_opaque = false;
    let mut any_transparent = false;
    for y in y0..y1 {
        for x in x0..x1 {
            if img.get_pixel(x, y).0[3] >= 128 {
                any_opaque = true;
            } else {
                any_transparent = true;
            }
            if any_opaque && any_transparent {
                return FaceCover::Mixed;
            }
        }
    }
    if any_opaque { FaceCover::Opaque } else { FaceCover::Empty }
}

/// Append one MC cube's 6 faces to the mesh buffers. Geometry is in MC
/// pixels; UVs follow MC's box unwrap (top/bottom row + a side strip),
/// matching `net/minecraft/client/model/geom/builders` exactly.
fn append_cube(
    verts: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    cube: &McCube,
    img: &image::RgbaImage,
) {
    let (tex_w, tex_h) = (img.width(), img.height());
    let g = cube.inflate;
    let [ox, oy, oz] = cube.origin;
    let [sx, sy, sz] = cube.size;
    // Geometry extents in MC pixels (inflate grows every side). MC is
    // Y-down, so `y0` is the up side and `y1` the down side; `z0` is the
    // front side (-Z, where the mob looks).
    let x0 = ox - g;
    let y0 = oy - g;
    let z0 = oz - g;
    let x1 = ox + sx + g;
    let y1 = oy + sy + g;
    let z1 = oz + sz + g;

    // UV tiles use the *un-inflated* size (w,h,d), exactly like MC.
    let (w, h, d) = (sx, sy, sz);
    let (u0, v0) = (cube.uv[0] as f32, cube.uv[1] as f32);
    let (tw, th) = (tex_w as f32, tex_h as f32);
    let uv = |pu: f32, pv: f32| -> [f32; 2] { [pu / tw, pv / th] };
    // MC box unwrap, tile U/V pixel ranges (v split: side strip d..d+h,
    // top/bottom strip 0..d):
    //   right (0,d)  front (d,d)  left (d+w,d)  back (d+w+d,d)
    //   top   (d,0)               bottom (d+w,0)
    let (r_u0, r_u1) = (u0, u0 + d);
    let (f_u0, f_u1) = (u0 + d, u0 + d + w);
    let (l_u0, l_u1) = (u0 + d + w, u0 + d + w + d);
    let (b_u0, b_u1) = (u0 + d + w + d, u0 + d + w + d + w);
    let sv0 = v0 + d;
    let sv1 = v0 + d + h;
    let (t_u0, t_u1) = (u0 + d, u0 + d + w);
    let (bt_u0, bt_u1) = (u0 + d + w, u0 + d + w + w);
    let tv0 = v0;
    let tv1 = v0 + d;
    // mirror() flips each tile horizontally (swap its U min/max).
    let mu = |a: f32, b: f32| -> (f32, f32) { if cube.mirror { (b, a) } else { (a, b) } };

    // W141: classify each face's sheet rect once at bake. Fully-
    // transparent faces are dropped (5 of the classic chicken leg's 6
    // faces); partially-transparent faces additionally get the reverse
    // winding so the art reads from both sides (MC entityCutoutNoCull).
    let mut add =
        |corners: [[f32; 3]; 4], normal: [f32; 3], uvs: [[f32; 2]; 4], cover: FaceCover| {
            if cover == FaceCover::Empty {
                return;
            }
            let base = verts.len() as u32;
            for i in 0..4 {
                verts.push(MeshVertex::new(corners[i], normal).with_uv(uvs[i]).with_light(0.0, 1.0));
            }
            indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
            if cover == FaceCover::Mixed {
                indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        };

    // The six faces are MC's `build_box_uv_mesh` convention expressed in
    // MC model space (front = -Z, right = +X, up = -Y) so that after the
    // shared MC->engine flip in `bone_matrix` the result matches the
    // engine's proven hand-built mob meshes (front = +X, up = +Y). Corner
    // order per face is (bottom-left, bottom-right, top-right, top-left) as
    // seen from outside; UVs pair (uL,vBot),(uR,vBot),(uR,vTop),(uL,vTop).

    // Front face (MC -Z): the mob's face / eyes.
    let (fa, fb) = mu(f_u0, f_u1);
    add(
        [[x1, y1, z0], [x0, y1, z0], [x0, y0, z0], [x1, y0, z0]],
        [0.0, 0.0, -1.0],
        [uv(fa, sv1), uv(fb, sv1), uv(fb, sv0), uv(fa, sv0)],
        face_cover(img, f_u0, f_u1, sv0, sv1),
    );
    // Back face (MC +Z).
    let (ba, bb) = mu(b_u0, b_u1);
    add(
        [[x0, y1, z1], [x1, y1, z1], [x1, y0, z1], [x0, y0, z1]],
        [0.0, 0.0, 1.0],
        [uv(ba, sv1), uv(bb, sv1), uv(bb, sv0), uv(ba, sv0)],
        face_cover(img, b_u0, b_u1, sv0, sv1),
    );
    // Right face (MC +X).
    let (ra, rb) = mu(r_u0, r_u1);
    add(
        [[x1, y1, z1], [x1, y1, z0], [x1, y0, z0], [x1, y0, z1]],
        [1.0, 0.0, 0.0],
        [uv(ra, sv1), uv(rb, sv1), uv(rb, sv0), uv(ra, sv0)],
        face_cover(img, r_u0, r_u1, sv0, sv1),
    );
    // Left face (MC -X).
    let (la, lb) = mu(l_u0, l_u1);
    add(
        [[x0, y1, z0], [x0, y1, z1], [x0, y0, z1], [x0, y0, z0]],
        [-1.0, 0.0, 0.0],
        [uv(la, sv1), uv(lb, sv1), uv(lb, sv0), uv(la, sv0)],
        face_cover(img, l_u0, l_u1, sv0, sv1),
    );
    // Top face (MC -Y = up).
    let (ta, tb) = mu(t_u0, t_u1);
    add(
        [[x1, y0, z1], [x1, y0, z0], [x0, y0, z0], [x0, y0, z1]],
        [0.0, -1.0, 0.0],
        [uv(ta, tv1), uv(tb, tv1), uv(tb, tv0), uv(ta, tv0)],
        face_cover(img, t_u0, t_u1, tv0, tv1),
    );
    // Bottom face (MC +Y).
    let (bta, btb) = mu(bt_u0, bt_u1);
    add(
        [[x0, y1, z1], [x0, y1, z0], [x1, y1, z0], [x1, y1, z1]],
        [0.0, 1.0, 0.0],
        [uv(bta, tv1), uv(btb, tv1), uv(btb, tv0), uv(bta, tv0)],
        face_cover(img, bt_u0, bt_u1, tv0, tv1),
    );
}

/// World transform for a bone: entity pos + yaw, the MC->engine flip,
/// the bone pivot + (default + animated) rotation. Cube geometry is in
/// bone-local MC pixels, so this is the only place coordinates convert.
pub(crate) fn bone_matrix(
    world_pos: Vec3,
    yaw: f32,
    ground_offset: f32,
    bone: &Bone,
    pose: BonePose,
) -> Mat4 {
    // Entity placement: translate to world pos (feet on ground), yaw.
    let place = Mat4::from_translation(world_pos + Vec3::new(0.0, ground_offset, 0.0))
        * Mat4::from_rotation_y(yaw);
    // MC -> engine: flip X + Y, scale 1/16.
    let flip = Mat4::from_scale(Vec3::new(-MODEL_SCALE, -MODEL_SCALE, MODEL_SCALE));
    // Bone pose in MC pixels: move to pivot, then rotate about it.
    let rx = bone.rotation[0] + pose.extra_rotation[0];
    let ry = bone.rotation[1] + pose.extra_rotation[1];
    let rz = bone.rotation[2] + pose.extra_rotation[2];
    let bone_xform = Mat4::from_translation(Vec3::from(bone.pivot))
        * Mat4::from_quat(Quat::from_euler(glam::EulerRot::ZYX, rz, ry, rx));
    place * flip * bone_xform
}

/// Draw every bone of a baked model at the entity's world pos + yaw.
/// `pose_for` supplies the per-bone animation pose (default static).
pub(crate) fn draw(
    r: &mut Renderer,
    baked: &BakedModel,
    world_pos: Vec3,
    yaw: f32,
    tint: [f32; 4],
    mut pose_for: impl FnMut(&str) -> BonePose,
) {
    for bb in &baked.bones {
        let pose = pose_for(bb.bone.name);
        let m = bone_matrix(world_pos, yaw, baked.ground_offset, &bb.bone, pose);
        r.draw_mesh_pbr(bb.mesh, m.to_cols_array(), tint, 0.0, 0.9);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::FRAC_PI_2;

    fn uv_of(v: &MeshVertex) -> [f32; 2] {
        [v.uv_packed[0] as f32 / 65535.0, v.uv_packed[1] as f32 / 65535.0]
    }
    /// Fully-opaque white sheet - every face classifies `Opaque`, so the
    /// classic single-winding bake expectations hold unchanged.
    fn opaque_sheet(w: u32, h: u32) -> image::RgbaImage {
        image::RgbaImage::from_pixel(w, h, image::Rgba([255, 255, 255, 255]))
    }
    fn normal_of(v: &MeshVertex) -> [f32; 3] {
        [
            v.normal_pad[0] as f32 / 127.0,
            v.normal_pad[1] as f32 / 127.0,
            v.normal_pad[2] as f32 / 127.0,
        ]
    }
    /// (u_min,u_max,v_min,v_max) over a 4-vertex face, in pixels for the
    /// given texture size.
    fn uv_px_bbox(face: &[MeshVertex], tw: f32, th: f32) -> (f32, f32, f32, f32) {
        let us: Vec<f32> = face.iter().map(|v| uv_of(v)[0] * tw).collect();
        let vs: Vec<f32> = face.iter().map(|v| uv_of(v)[1] * th).collect();
        let mn = |xs: &[f32]| xs.iter().cloned().fold(f32::INFINITY, f32::min);
        let mx = |xs: &[f32]| xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        (mn(&us), mx(&us), mn(&vs), mx(&vs))
    }

    /// Each of the six faces must land on its MC box-unwrap tile (the bug
    /// that put the cow's eyes/horns on the wrong faces was exactly this).
    /// Cube = cow head main box: texOffs(0,0), size 8x8x6, texture 64x64.
    /// Tiles (px): right u0..6, front u6..14, left u14..20, back u20..28,
    /// all side v6..14; top u6..14, bottom u14..22, both v0..6.
    #[test]
    fn append_cube_uv_matches_mc_unwrap() {
        let cube = McCube::new([-4.0, -4.0, -6.0], [8.0, 8.0, 6.0], [0, 0]);
        let mut v = Vec::new();
        let mut i = Vec::new();
        append_cube(&mut v, &mut i, &cube, &opaque_sheet(64, 64));
        assert_eq!(v.len(), 24, "6 faces * 4 verts");
        assert_eq!(i.len(), 36, "6 faces * 2 tris");

        // Emission order: front, back, right, left, top, bottom.
        let eps = 0.05; // sub-pixel rounding from u16 packing.
        let about = |a: f32, b: f32| (a - b).abs() < eps;
        let check = |name: &str, f: &[MeshVertex], want: (f32, f32, f32, f32)| {
            let (u0, u1, v0, v1) = uv_px_bbox(f, 64.0, 64.0);
            assert!(
                about(u0, want.0) && about(u1, want.1) && about(v0, want.2) && about(v1, want.3),
                "{name} uv px = ({u0},{u1},{v0},{v1}) want {want:?}"
            );
        };
        check("front", &v[0..4], (6.0, 14.0, 6.0, 14.0));
        check("back", &v[4..8], (20.0, 28.0, 6.0, 14.0));
        check("right", &v[8..12], (0.0, 6.0, 6.0, 14.0));
        check("left", &v[12..16], (14.0, 20.0, 6.0, 14.0));
        check("top", &v[16..20], (6.0, 14.0, 0.0, 6.0));
        check("bottom", &v[20..24], (14.0, 22.0, 0.0, 6.0));
    }

    /// Faces sit on the right geometric planes with the right outward MC
    /// normals (front=-Z, back=+Z, right=+X, left=-X, top=-Y, bottom=+Y).
    #[test]
    fn append_cube_face_geometry() {
        let cube = McCube::new([-4.0, -4.0, -6.0], [8.0, 8.0, 6.0], [0, 0]);
        let mut v = Vec::new();
        let mut i = Vec::new();
        append_cube(&mut v, &mut i, &cube, &opaque_sheet(64, 64));
        // (x0,y0,z0)=(-4,-4,-6), (x1,y1,z1)=(4,4,0).
        let all = |f: &[MeshVertex], axis: usize, val: f32| {
            f.iter().all(|vt| (vt.position[axis] - val).abs() < 1e-4)
        };
        assert!(all(&v[0..4], 2, -6.0), "front on z0");
        assert!(all(&v[4..8], 2, 0.0), "back on z1");
        assert!(all(&v[8..12], 0, 4.0), "right on x1");
        assert!(all(&v[12..16], 0, -4.0), "left on x0");
        assert!(all(&v[16..20], 1, -4.0), "top on y0 (MC up)");
        assert!(all(&v[20..24], 1, 4.0), "bottom on y1");
        // Normals.
        let n = |f: &[MeshVertex]| normal_of(&f[0]);
        assert_eq!(n(&v[0..4]).map(|x| x.round()), [0.0, 0.0, -1.0]);
        assert_eq!(n(&v[8..12]).map(|x| x.round()), [1.0, 0.0, 0.0]);
        assert_eq!(n(&v[16..20]).map(|x| x.round()), [0.0, -1.0, 0.0]);
    }

    /// The render layer passes `mob.yaw - PI/2`; at mob.yaw = 0 that must
    /// rotate the MC front (-Z) onto the engine's forward (+X) and keep MC
    /// "up" (-Y) pointing engine-up (+Y). This is the orientation half of
    /// the cow's udder/horn fix, verifiable without a GPU.
    #[test]
    fn bone_matrix_orients_mc_front_to_engine_plus_x() {
        let bone = Bone { name: "b", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: &[] };
        let m = bone_matrix(Vec3::ZERO, -FRAC_PI_2, 0.0, &bone, BonePose::default());
        let front = m.transform_point3(Vec3::new(0.0, 0.0, -1.0));
        assert!(
            front.x > 0.0 && front.x.abs() > front.z.abs() && front.x.abs() > front.y.abs(),
            "MC front (-Z) should map to engine +X, got {front:?}"
        );
        let up = m.transform_point3(Vec3::new(0.0, -1.0, 0.0));
        assert!(up.y > 0.0 && up.y.abs() > up.x.abs(), "MC up (-Y) -> engine +Y, got {up:?}");
    }

    /// The MC->engine flip must preserve winding (det +1), or every face
    /// would be back-face culled.
    #[test]
    fn flip_preserves_winding() {
        let flip = Mat4::from_scale(Vec3::new(-MODEL_SCALE, -MODEL_SCALE, MODEL_SCALE));
        assert!(flip.determinant() > 0.0, "flip det must be positive");
    }

    /// W141: the classic chicken-leg situation - a cube whose sheet art
    /// is one opaque column on the back tile, everything else transparent.
    /// Fully-transparent faces must not be emitted at all, and the one
    /// mixed face must carry BOTH windings (entityCutoutNoCull parity).
    #[test]
    fn transparent_faces_skipped_mixed_faces_double_wound() {
        // Leg cube: texOffs(26,0), size 3x5x3 - the chicken's real data.
        let cube = McCube::new([-1.0, 0.0, -3.0], [3.0, 5.0, 3.0], [26, 0]);
        let mut img = image::RgbaImage::from_pixel(64, 32, image::Rgba([0, 0, 0, 0]));
        // Opaque column inside the BACK tile (u 35..38, v 3..8): x=36.
        for y in 3..8 {
            img.put_pixel(36, y, image::Rgba([224, 187, 105, 255]));
        }
        let mut v = Vec::new();
        let mut i = Vec::new();
        append_cube(&mut v, &mut i, &cube, &img);
        // Only the back face survives: 4 verts, 2 windings = 12 indices.
        assert_eq!(v.len(), 4, "5 fully-transparent faces dropped");
        assert_eq!(i.len(), 12, "mixed face = front + reverse winding");
        // Both windings reference the same 4 vertices.
        assert!(i.iter().all(|&ix| ix < 4));
    }

    /// W141: an all-opaque cube keeps the classic 24/36 single-winding
    /// bake - no hidden extra geometry on solid mobs.
    #[test]
    fn opaque_cube_keeps_single_winding() {
        let cube = McCube::new([0.0, 0.0, 0.0], [4.0, 4.0, 4.0], [0, 0]);
        let mut v = Vec::new();
        let mut i = Vec::new();
        append_cube(&mut v, &mut i, &cube, &opaque_sheet(64, 64));
        assert_eq!(v.len(), 24);
        assert_eq!(i.len(), 36);
    }

    /// W141: face_cover classification basics, incl. degenerate rects.
    #[test]
    fn face_cover_classifies() {
        let mut img = image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 255, 255, 255]));
        img.put_pixel(1, 1, image::Rgba([0, 0, 0, 0]));
        assert_eq!(face_cover(&img, 0.0, 8.0, 0.0, 8.0), FaceCover::Mixed);
        assert_eq!(face_cover(&img, 4.0, 8.0, 4.0, 8.0), FaceCover::Opaque);
        assert_eq!(face_cover(&img, 1.0, 2.0, 1.0, 2.0), FaceCover::Empty);
        // Zero-area + fully out-of-sheet rects are Empty (skipped).
        assert_eq!(face_cover(&img, 3.0, 3.0, 0.0, 8.0), FaceCover::Empty);
        assert_eq!(face_cover(&img, 100.0, 104.0, 0.0, 4.0), FaceCover::Empty);
    }
}
