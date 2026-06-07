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

pub(crate) mod cow;

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
    pub(crate) const fn mirrored(mut self) -> Self {
        self.mirror = true;
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
    let albedo = r.make_mesh_albedo(img.width(), img.height(), img);
    // UVs are normalized by the ACTUAL texture size (MC-asset-driven), not
    // a hardcoded model constant - so the same `texOffs` data is correct
    // whatever resolution the pack ships.
    let (tex_w, tex_h) = (img.width(), img.height());
    let mut bones = Vec::with_capacity(model.bones.len());
    let mut lowest_world_y = 0.0_f32;
    for bone in model.bones {
        let (verts, indices) = bake_bone(bone, tex_w, tex_h);
        let mesh = r.create_mesh(bone.name, &verts, &indices);
        r.bind_mesh_albedo_arc(mesh, albedo.clone());
        bones.push(BakedBone { bone: *bone, mesh });
        // Track the lowest point (largest MC +Y, since Y is down) across
        // all cubes including the bone pivot, to derive the feet offset.
        for c in bone.cubes {
            let bottom_mc = bone.pivot[1] + c.origin[1] + c.size[1] + c.inflate;
            // world Y = -mc_y * scale, so a larger MC y is a lower world y.
            let world_y = -bottom_mc * MODEL_SCALE;
            lowest_world_y = lowest_world_y.min(world_y);
        }
    }
    BakedModel { bones, ground_offset: -lowest_world_y }
}

/// Bake one bone's cubes into a single mesh, expressed in bone-local MC
/// pixels (cube coords as-is; the bone pivot is applied at draw time).
fn bake_bone(bone: &Bone, tex_w: u32, tex_h: u32) -> (Vec<MeshVertex>, Vec<u32>) {
    let mut verts: Vec<MeshVertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for cube in bone.cubes {
        append_cube(&mut verts, &mut indices, cube, tex_w, tex_h);
    }
    (verts, indices)
}

/// Append one MC cube's 6 faces to the mesh buffers. Geometry is in MC
/// pixels; UVs follow MC's box unwrap (top/bottom row + a side strip),
/// matching `net/minecraft/client/model/geom/builders` exactly.
fn append_cube(
    verts: &mut Vec<MeshVertex>,
    indices: &mut Vec<u32>,
    cube: &McCube,
    tex_w: u32,
    tex_h: u32,
) {
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

    let mut add = |corners: [[f32; 3]; 4], normal: [f32; 3], uvs: [[f32; 2]; 4]| {
        let base = verts.len() as u32;
        for i in 0..4 {
            verts.push(MeshVertex::new(corners[i], normal).with_uv(uvs[i]).with_light(0.0, 1.0));
        }
        indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
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
    );
    // Back face (MC +Z).
    let (ba, bb) = mu(b_u0, b_u1);
    add(
        [[x0, y1, z1], [x1, y1, z1], [x1, y0, z1], [x0, y0, z1]],
        [0.0, 0.0, 1.0],
        [uv(ba, sv1), uv(bb, sv1), uv(bb, sv0), uv(ba, sv0)],
    );
    // Right face (MC +X).
    let (ra, rb) = mu(r_u0, r_u1);
    add(
        [[x1, y1, z1], [x1, y1, z0], [x1, y0, z0], [x1, y0, z1]],
        [1.0, 0.0, 0.0],
        [uv(ra, sv1), uv(rb, sv1), uv(rb, sv0), uv(ra, sv0)],
    );
    // Left face (MC -X).
    let (la, lb) = mu(l_u0, l_u1);
    add(
        [[x0, y1, z0], [x0, y1, z1], [x0, y0, z1], [x0, y0, z0]],
        [-1.0, 0.0, 0.0],
        [uv(la, sv1), uv(lb, sv1), uv(lb, sv0), uv(la, sv0)],
    );
    // Top face (MC -Y = up).
    let (ta, tb) = mu(t_u0, t_u1);
    add(
        [[x1, y0, z1], [x1, y0, z0], [x0, y0, z0], [x0, y0, z1]],
        [0.0, -1.0, 0.0],
        [uv(ta, tv1), uv(tb, tv1), uv(tb, tv0), uv(ta, tv0)],
    );
    // Bottom face (MC +Y).
    let (bta, btb) = mu(bt_u0, bt_u1);
    add(
        [[x0, y1, z1], [x0, y1, z0], [x1, y1, z0], [x1, y1, z1]],
        [0.0, 1.0, 0.0],
        [uv(bta, tv1), uv(btb, tv1), uv(btb, tv0), uv(bta, tv0)],
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
