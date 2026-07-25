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

pub(crate) mod animation;
pub(crate) mod cat;
pub(crate) mod chest;
pub(crate) mod chicken;
pub(crate) mod cow;
pub(crate) mod bat;
pub(crate) mod bee;
pub(crate) mod fox;
pub(crate) mod goat;
pub(crate) mod parrot;
pub(crate) mod phantom;
pub(crate) mod squid;
pub(crate) mod turtle;
pub(crate) mod enderman;
pub(crate) mod silverfish;
pub(crate) mod blaze;
pub(crate) mod creeper;
pub(crate) mod guardian;
pub(crate) mod illager;
pub(crate) mod donkey;
pub(crate) mod ghast;
pub(crate) mod horse;
pub(crate) mod iron_golem;
pub(crate) mod magma_cube;
pub(crate) mod minecart;
pub(crate) mod pig;
pub(crate) mod piglin;
pub(crate) mod polar_bear;
pub(crate) mod rabbit;
pub(crate) mod sheep;
pub(crate) mod skeleton;
pub(crate) mod spider;
pub(crate) mod villager;
pub(crate) mod wolf;
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
    /// W162: parent bone name, or `None` for a root bone. Mirrors
    /// MC's `addOrReplaceChild` tree: a child's `pivot` + `rotation`
    /// are RELATIVE to its parent, and its cubes inherit the parent's
    /// accumulated transform. Flat models leave every bone at `None`.
    pub parent: Option<&'static str>,
    /// `PartPose.offset` - the bone's pivot, relative to its parent
    /// (or the entity origin for a root bone).
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
    /// W327: MC `MeshTransformer.scaling(..)` - a whole-model scale
    /// applied AFTER the UV unwrap (box sizes keep driving the
    /// texture rects; only world-space geometry grows). 1.0 for
    /// most mobs; the ghast ships 4.5, the magma cube 2.0.
    pub scale: f32,
    /// W327: the MC `LayerDefinition.create(mesh, W, H)` texture
    /// grid the model's texOffs values live in. `None` = normalize
    /// UVs by the loaded file's own dimensions (correct whenever
    /// the pack ships the sheet at grid size, which every mob so
    /// far did). `Some` pins the grid so an HD sheet (the 26.1.2
    /// ghast ships 128x64 art on a 64x32 grid) still maps texOffs
    /// proportionally.
    pub tex_grid: Option<(u32, u32)>,
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
    /// W327: the model's whole-body scale (EntityModel::scale),
    /// folded into the MC->world flip matrix at draw time.
    pub model_scale: f32,
}

/// Per-frame animation pose for a bone, layered on top of its default
/// `PartPose`. Empty = static. (Wired per-mob by the render layer:
/// head look, leg swing, etc.)
#[derive(Clone, Copy, Default)]
pub(crate) struct BonePose {
    pub extra_rotation: [f32; 3],
    /// W336: MC-pixel translation added to the pivot (ModelPart
    /// x/y/z pose writes - the magma cube squish spread).
    pub extra_offset: [f32; 3],
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
        let (verts, indices) = bake_bone(bone, img, model.tex_grid);
        let mesh = r.create_mesh(bone.name, &verts, &indices);
        r.bind_mesh_albedo_arc(mesh, albedo.clone());
        bones.push(BakedBone { bone: *bone, mesh });
        // Track the lowest world point (the feet) to derive the ground
        // offset. W162: use the bone's ACCUMULATED default transform
        // (its full parent chain) so nested limbs (rabbit haunches,
        // bear legs) are measured at their real tips, not their local
        // extents. world Y = -mc_y * scale, so the largest MC y is the
        // lowest world y.
        let acc = default_accumulated(model.bones, bone);
        for c in bone.cubes {
            let [ox, oy, oz] = c.origin;
            let [sx, sy, sz] = c.size;
            let g = c.inflate;
            for &cx in &[ox - g, ox + sx + g] {
                for &cy in &[oy - g, oy + sy + g] {
                    for &cz in &[oz - g, oz + sz + g] {
                        let p = acc.transform_point3(Vec3::new(cx, cy, cz));
                        lowest_world_y = lowest_world_y.min(-p.y * MODEL_SCALE);
                    }
                }
            }
        }
    }
    BakedModel {
        bones,
        // W327: the ground scan ran in unscaled MC space; the
        // model-level scale grows the world extent linearly.
        ground_offset: -lowest_world_y * model.scale,
        model_scale: model.scale,
    }
}

/// W162: a bone's own local transform in MC pixel space:
/// `translate(pivot) * rotate(rotation + pose.extra)`. The parent
/// chain is composed on top of this by `draw` / `default_accumulated`.
fn bone_local(pivot: [f32; 3], rotation: [f32; 3], pose: &BonePose) -> Mat4 {
    let rx = rotation[0] + pose.extra_rotation[0];
    let ry = rotation[1] + pose.extra_rotation[1];
    let rz = rotation[2] + pose.extra_rotation[2];
    Mat4::from_translation(Vec3::from(pivot) + Vec3::from(pose.extra_offset))
        * Mat4::from_quat(Quat::from_euler(glam::EulerRot::ZYX, rz, ry, rx))
}

/// W162: a bone's accumulated DEFAULT (un-animated) transform - its
/// own local times every ancestor's local, root-first. Used by the
/// ground-offset scan in `bake`. Parent lookup is by name over the
/// model's flat bone list; a missing / cyclic parent stops the walk.
fn default_accumulated(all: &[Bone], bone: &Bone) -> Mat4 {
    let mut acc = bone_local(bone.pivot, bone.rotation, &BonePose::default());
    let mut parent = bone.parent;
    let mut guard = 0;
    while let Some(pn) = parent {
        guard += 1;
        if guard > 16 {
            break;
        }
        match all.iter().find(|b| b.name == pn) {
            Some(p) => {
                acc = bone_local(p.pivot, p.rotation, &BonePose::default()) * acc;
                parent = p.parent;
            }
            None => break,
        }
    }
    acc
}

/// Bake one bone's cubes into a single mesh, expressed in bone-local MC
/// pixels (cube coords as-is; the bone pivot is applied at draw time).
fn bake_bone(
    bone: &Bone,
    img: &image::RgbaImage,
    tex_grid: Option<(u32, u32)>,
) -> (Vec<MeshVertex>, Vec<u32>) {
    let mut verts: Vec<MeshVertex> = Vec::new();
    let mut indices: Vec<u32> = Vec::new();
    for cube in bone.cubes {
        append_cube(&mut verts, &mut indices, cube, img, tex_grid);
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
    tex_grid: Option<(u32, u32)>,
) {
    // W327: texOffs live in the model's LayerDefinition grid. UVs
    // normalize by that grid; the alpha scan below maps grid pixels
    // to FILE pixels through px_scale (an HD sheet is an integer
    // upscale of the grid).
    let (tex_w, tex_h) = tex_grid.unwrap_or((img.width(), img.height()));
    let px_scale = (
        img.width() as f32 / tex_w as f32,
        img.height() as f32 / tex_h as f32,
    );
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
        face_cover(img, f_u0 * px_scale.0, f_u1 * px_scale.0, sv0 * px_scale.1, sv1 * px_scale.1),
    );
    // Back face (MC +Z).
    let (ba, bb) = mu(b_u0, b_u1);
    add(
        [[x0, y1, z1], [x1, y1, z1], [x1, y0, z1], [x0, y0, z1]],
        [0.0, 0.0, 1.0],
        [uv(ba, sv1), uv(bb, sv1), uv(bb, sv0), uv(ba, sv0)],
        face_cover(img, b_u0 * px_scale.0, b_u1 * px_scale.0, sv0 * px_scale.1, sv1 * px_scale.1),
    );
    // Right face (MC +X).
    let (ra, rb) = mu(r_u0, r_u1);
    add(
        [[x1, y1, z1], [x1, y1, z0], [x1, y0, z0], [x1, y0, z1]],
        [1.0, 0.0, 0.0],
        [uv(ra, sv1), uv(rb, sv1), uv(rb, sv0), uv(ra, sv0)],
        face_cover(img, r_u0 * px_scale.0, r_u1 * px_scale.0, sv0 * px_scale.1, sv1 * px_scale.1),
    );
    // Left face (MC -X).
    let (la, lb) = mu(l_u0, l_u1);
    add(
        [[x0, y1, z0], [x0, y1, z1], [x0, y0, z1], [x0, y0, z0]],
        [-1.0, 0.0, 0.0],
        [uv(la, sv1), uv(lb, sv1), uv(lb, sv0), uv(la, sv0)],
        face_cover(img, l_u0 * px_scale.0, l_u1 * px_scale.0, sv0 * px_scale.1, sv1 * px_scale.1),
    );
    // Top face (MC -Y = up).
    let (ta, tb) = mu(t_u0, t_u1);
    add(
        [[x1, y0, z1], [x1, y0, z0], [x0, y0, z0], [x0, y0, z1]],
        [0.0, -1.0, 0.0],
        [uv(ta, tv1), uv(tb, tv1), uv(tb, tv0), uv(ta, tv0)],
        face_cover(img, t_u0 * px_scale.0, t_u1 * px_scale.0, tv0 * px_scale.1, tv1 * px_scale.1),
    );
    // Bottom face (MC +Y).
    let (bta, btb) = mu(bt_u0, bt_u1);
    add(
        [[x0, y1, z1], [x0, y1, z0], [x1, y1, z0], [x1, y1, z1]],
        [0.0, 1.0, 0.0],
        [uv(bta, tv1), uv(btb, tv1), uv(btb, tv0), uv(bta, tv0)],
        face_cover(img, bt_u0 * px_scale.0, bt_u1 * px_scale.0, tv0 * px_scale.1, tv1 * px_scale.1),
    );
}

/// World transform for a SINGLE root bone: entity pos + yaw, the
/// MC->engine flip, the bone pivot + (default + animated) rotation.
/// W162: the live `draw` path composes the parent chain instead;
/// this single-bone form is retained for the sign-convention unit
/// tests (root-only bones, where it equals the chain result).
#[cfg(test)]
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
    // MC -> engine: flip X + Y, scale 1/16. (Model-level scale is a
    // draw_with_root concern - this legacy helper serves 1.0-scale
    // paths only.)
    let flip = Mat4::from_scale(Vec3::new(-MODEL_SCALE, -MODEL_SCALE, MODEL_SCALE));
    // Bone pose in MC pixels: move to pivot, then rotate about it.
    let rx = bone.rotation[0] + pose.extra_rotation[0];
    let ry = bone.rotation[1] + pose.extra_rotation[1];
    let rz = bone.rotation[2] + pose.extra_rotation[2];
    let bone_xform =
        Mat4::from_translation(Vec3::from(bone.pivot) + Vec3::from(pose.extra_offset))
            * Mat4::from_quat(Quat::from_euler(glam::EulerRot::ZYX, rz, ry, rx));
    place * flip * bone_xform
}

/// Draw every bone of a baked model at the entity's world pos + yaw.
/// `pose_for` supplies the per-bone animation pose (default static).
///
/// W162: bones compose through their parent chain (MC ModelPart
/// tree). Each bone's local matrix (pivot + default + animated
/// rotation) is cached once, then a bone's world transform is
/// `place * flip * (root_local * ... * own_local)` so animating a
/// parent (or its default rotation) carries its children.
pub(crate) fn draw(
    r: &mut Renderer,
    baked: &BakedModel,
    world_pos: Vec3,
    yaw: f32,
    tint: [f32; 4],
    pose_for: impl FnMut(&str) -> BonePose,
) {
    draw_scaled(r, baked, world_pos, yaw, 1.0, tint, pose_for);
}

/// W291: `draw` with a uniform scale about the FEET (world_pos) -
/// the baby-mob render (MC babies draw at half scale). ground_offset
/// scales with the model so a shrunk mob still stands on the ground.
pub(crate) fn draw_scaled(
    r: &mut Renderer,
    baked: &BakedModel,
    world_pos: Vec3,
    yaw: f32,
    scale: f32,
    tint: [f32; 4],
    pose_for: impl FnMut(&str) -> BonePose,
) {
    let place = Mat4::from_translation(world_pos)
        * Mat4::from_scale(Vec3::splat(scale))
        * Mat4::from_translation(Vec3::new(0.0, baked.ground_offset, 0.0))
        * Mat4::from_rotation_y(yaw);
    draw_with_root(r, baked, place, tint, pose_for);
}

/// W250: the same bone walk with a caller-composed root transform.
/// The spawner-cage display mob needs MC SpawnerRenderer's
/// translate/spin/tilt/shrink chain around the whole model, which the
/// pos+yaw form can't express. The root receives model-local space
/// with the feet at the origin AFTER `ground_offset` - callers that
/// bypass `draw` must fold `baked.ground_offset` in themselves.
pub(crate) fn draw_with_root(
    r: &mut Renderer,
    baked: &BakedModel,
    place: Mat4,
    tint: [f32; 4],
    mut pose_for: impl FnMut(&str) -> BonePose,
) {
    // W327: fold the model-level scale into the MC->world flip so
    // every bone transform (pivots included, still in MC pixels)
    // scales uniformly.
    let s = MODEL_SCALE * baked.model_scale;
    let flip = Mat4::from_scale(Vec3::new(-s, -s, s));
    // Cache each bone's local matrix (pose included) once, indexed
    // by position so the parent walk is a cheap slice lookup.
    let locals: Vec<Mat4> = baked
        .bones
        .iter()
        .map(|bb| {
            let p = pose_for(bb.bone.name);
            bone_local(bb.bone.pivot, bb.bone.rotation, &p)
        })
        .collect();
    for (i, bb) in baked.bones.iter().enumerate() {
        // Accumulate own local times every ancestor (root-first).
        let mut acc = locals[i];
        let mut parent = bb.bone.parent;
        let mut guard = 0;
        while let Some(pn) = parent {
            guard += 1;
            if guard > 16 {
                break;
            }
            match baked.bones.iter().position(|b| b.bone.name == pn) {
                Some(pi) => {
                    acc = locals[pi] * acc;
                    parent = baked.bones[pi].bone.parent;
                }
                None => break,
            }
        }
        let m = place * flip * acc;
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
        append_cube(&mut v, &mut i, &cube, &opaque_sheet(64, 64), None);
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
        append_cube(&mut v, &mut i, &cube, &opaque_sheet(64, 64), None);
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
        let bone = Bone { parent: None, name: "b", pivot: [0.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: &[] };
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
        let flip =
            Mat4::from_scale(Vec3::new(-MODEL_SCALE, -MODEL_SCALE, MODEL_SCALE));
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
        append_cube(&mut v, &mut i, &cube, &img, None);
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
        append_cube(&mut v, &mut i, &cube, &opaque_sheet(64, 64), None);
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

    /// Two transforms are equal iff they agree on a probe point.
    fn same_xform(a: Mat4, b: Mat4, tol: f32) -> bool {
        let p = Vec3::new(1.3, -2.1, 0.7);
        (a.transform_point3(p) - b.transform_point3(p)).length() < tol
    }

    /// W162: a root bone (parent None) must accumulate to exactly its
    /// own local transform - so every FLAT model renders identically
    /// to the pre-W162 single-bone path.
    #[test]
    fn w162_root_bone_accumulates_to_own_local() {
        let model = [Bone {
            parent: None, name: "root", pivot: [1.0, 2.0, 3.0],
            rotation: [0.3, -0.4, 0.5], cubes: &[],
        }];
        let acc = default_accumulated(&model, &model[0]);
        let own = bone_local([1.0, 2.0, 3.0], [0.3, -0.4, 0.5], &BonePose::default());
        assert!(same_xform(acc, own, 1e-5), "root acc must equal its own local");
    }

    /// W162: regression guard - EVERY bone of a real flat model
    /// (the zombie, all parents None) accumulates to its own local.
    /// If this breaks, flat mobs silently shifted.
    #[test]
    fn w162_flat_model_is_unchanged() {
        for b in super::zombie::MODEL.bones {
            assert!(b.parent.is_none(), "zombie is a flat model");
            let acc = default_accumulated(super::zombie::MODEL.bones, b);
            let own = bone_local(b.pivot, b.rotation, &BonePose::default());
            assert!(same_xform(acc, own, 1e-5), "{} drifted under W162", b.name);
        }
    }

    /// W162: a child bone composes as `parent_local * child_local`.
    /// Concretely: a parent pivoted at y=10 and rotated +90deg about
    /// X turns a child offset of +5 in Z into a -5 in Y, so the
    /// child origin lands at (0, 5, 0).
    #[test]
    fn w162_child_composes_through_parent() {
        let model = [
            Bone {
                parent: None, name: "p", pivot: [0.0, 10.0, 0.0],
                rotation: [FRAC_PI_2, 0.0, 0.0], cubes: &[],
            },
            Bone {
                parent: Some("p"), name: "c", pivot: [0.0, 0.0, 5.0],
                rotation: [0.0, 0.0, 0.0], cubes: &[],
            },
        ];
        let acc = default_accumulated(&model, &model[1]);
        // Composition order matches MC: parent first, then child.
        let expected = bone_local([0.0, 10.0, 0.0], [FRAC_PI_2, 0.0, 0.0], &BonePose::default())
            * bone_local([0.0, 0.0, 5.0], [0.0, 0.0, 0.0], &BonePose::default());
        assert!(same_xform(acc, expected, 1e-4), "child != parent_local * child_local");
        // Tangible: the child's own origin lands at (0, 5, 0).
        let origin = acc.transform_point3(Vec3::ZERO);
        assert!(
            (origin - Vec3::new(0.0, 5.0, 0.0)).length() < 1e-4,
            "nested child origin should be (0,5,0), got {origin:?}"
        );
    }

    /// W162: a missing / unknown parent name stops the walk instead of
    /// looping forever - the orphaned bone just uses its own local.
    #[test]
    fn w162_unknown_parent_falls_back_to_own_local() {
        let model = [Bone {
            parent: Some("does_not_exist"), name: "orphan",
            pivot: [2.0, 0.0, 0.0], rotation: [0.0, 0.0, 0.0], cubes: &[],
        }];
        let acc = default_accumulated(&model, &model[0]);
        let own = bone_local([2.0, 0.0, 0.0], [0.0, 0.0, 0.0], &BonePose::default());
        assert!(same_xform(acc, own, 1e-5), "orphan bone must not panic / loop");
    }
}
