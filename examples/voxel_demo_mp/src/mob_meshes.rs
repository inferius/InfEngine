//! Wave 19.1 - Per-body-part UV-mapped cube meshes for MC mob skins.
//!
//! Each `MobPart` variant identifies one box in the MC entity-texture
//! UV atlas. `build_mob_part_mesh` turns a `MobPart` into a 24-vertex
//! / 36-index cube mesh (same vertex layout as `build_sheep_head_mesh`
//! in `main.rs`) with per-face UVs mapped to the canonical MC box-UV
//! unwrap for that part.
//!
//! ## MC box-UV layout
//!
//! For a cube of size `w x h x d` at texture origin `(u0, v0)` on a
//! `tw x th` pixel texture (y-down convention):
//!
//! ```text
//!   (u0+d,       v0)        top    w x d   (upper-left of atlas strip)
//!   (u0+d+w,     v0)        bottom w x d
//!   (u0,         v0+d)      right  d x h
//!   (u0+d,       v0+d)      front  w x h
//!   (u0+d+w,     v0+d)      left   d x h
//!   (u0+d+w+d,   v0+d)      back   w x h
//! ```
//!
//! Source references (MC 26.1.2):
//! - `net/minecraft/client/model/HumanoidModel.java`
//!   (zombie / skeleton / humanoid layout)
//! - `net/minecraft/client/model/QuadrupedModel.java`
//!   (pig / cow base body + legs)
//! - `net/minecraft/client/model/animal/pig/PigModel.java`
//! - `net/minecraft/client/model/animal/cow/CowModel.java`
//! - `net/minecraft/client/model/animal/chicken/AdultChickenModel.java`
//! - `net/minecraft/client/model/monster/spider/SpiderModel.java`
//! - `net/minecraft/client/model/monster/skeleton/SkeletonModel.java`

use infengine_render::{MeshHandle, MeshVertex};

// ---------------------------------------------------------------------------
// MobPart enum
// ---------------------------------------------------------------------------

/// One renderable box of a MC mob model. The UV constants here come
/// directly from `texOffs(u, v).addBox(...)` calls in the Java
/// model sources listed in the module doc comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // All variants used by W19.2 / W19.3 dispatch.
pub(crate) enum MobPart {
    // ---- Sheep (64x32 entity texture) -----------------------------------
    // Head: texOffs(0,0) addBox size [6,6,8]. Reuses existing
    // build_sheep_head_mesh; these constants are here for completeness.
    SheepHead,
    // Body: texOffs(28,8) addBox size [8,16,6] (base quadruped body).
    SheepBody,

    // ---- Zombie / humanoid (64x64 entity texture) -----------------------
    // Source: HumanoidModel.java
    // head    texOffs(0,0)   size [8,8,8]
    // body    texOffs(16,16) size [8,12,4]
    // right_arm texOffs(40,16) size [4,12,4]
    // left_arm  texOffs(40,16) size [4,12,4]  (mirrored in MC, same uv)
    // right_leg texOffs(0,16)  size [4,12,4]
    // left_leg  texOffs(16,48) size [4,12,4]  (mirrored, different uv)
    ZombieHead,
    ZombieBody,
    ZombieLeftLeg,
    ZombieRightLeg,
    ZombieLeftArm,
    ZombieRightArm,

    // ---- Chicken (64x32 entity texture) ---------------------------------
    // Source: AdultChickenModel.java
    // head texOffs(0,0)  addBox size [4,6,3]
    // body texOffs(0,9)  addBox size [6,8,6] (body is rotated 90deg in MC)
    // leg  texOffs(26,0) addBox size [3,5,3]  (both legs share same UV)
    ChickenHead,
    ChickenBody,
    ChickenLeftLeg,
    ChickenRightLeg,

    // ---- Pig (64x64 entity texture) -------------------------------------
    // Source: PigModel.java -> QuadrupedModel.createBodyMesh(legSize=6)
    // head texOffs(0,0)  addBox size [8,8,8]
    // body texOffs(28,8) addBox size [10,16,8] (rotated 90 in MC)
    // leg  texOffs(0,16) addBox size [4,6,4]
    PigHead,
    PigBody,
    PigLeftFrontLeg,
    PigRightFrontLeg,
    PigLeftBackLeg,
    PigRightBackLeg,

    // ---- Cow (64x64 entity texture) -------------------------------------
    // Source: CowModel.java -> QuadrupedModel.createBodyMesh(legSize=12)
    // head texOffs(0,0)   addBox size [8,8,6]
    // body texOffs(18,4)  addBox size [12,18,10] (rotated 90 in MC)
    // leg  texOffs(0,16)  addBox size [4,12,4]
    CowHead,
    CowBody,
    CowLeftFrontLeg,
    CowRightFrontLeg,
    CowLeftBackLeg,
    CowRightBackLeg,

    // ---- Skeleton (64x32 entity texture) --------------------------------
    // Source: SkeletonModel.java -> HumanoidModel.createMesh (same layout
    // as zombie but on a 64x32 texture; arms/legs use thinner boxes)
    // head     texOffs(0,0)   size [8,8,8]
    // body     texOffs(16,16) size [8,12,4]
    // right_arm texOffs(40,16) size [2,12,2]  (thinner than zombie)
    // left_arm  texOffs(40,16) size [2,12,2]
    // right_leg texOffs(0,16)  size [2,12,2]
    // left_leg  texOffs(0,16)  size [2,12,2]  (mirrored, same uv on 64x32)
    SkeletonHead,
    SkeletonBody,
    SkeletonLeftLeg,
    SkeletonRightLeg,
    SkeletonLeftArm,
    SkeletonRightArm,

    // ---- Spider (64x32 entity texture) ----------------------------------
    // Source: SpiderModel.java
    // head  texOffs(32,4) addBox size [8,8,8]
    // body0 texOffs(0,0)  addBox size [6,6,6]   (thorax)
    // body1 texOffs(0,12) addBox size [10,8,12]  (abdomen)
    // leg   texOffs(18,0) addBox size [16,2,2]   (all 8 legs share UV)
    SpiderHead,
    SpiderBody0,
    SpiderBody1,
    SpiderLeg,
}

// ---------------------------------------------------------------------------
// UV spec per part
// ---------------------------------------------------------------------------

/// MC box-UV descriptor: pixel origin (u0, v0) and cube dimensions
/// (w, h, d) in texture-pixel units.
#[allow(dead_code)] // fields read via pattern destructure in tests + W19.2
struct BoxUv {
    u0: u32,
    v0: u32,
    /// Width  of the box face (X dimension of the box in MC model space).
    w: u32,
    /// Height of the box face (Y dimension of the box in MC model space).
    h: u32,
    /// Depth  of the box face (Z dimension of the box in MC model space).
    d: u32,
}

impl MobPart {
    /// Return the MC box-UV spec for this part. The pixel values are
    /// absolute coordinates inside the entity texture (top-left = (0,0)).
    #[allow(dead_code)] // W19.2 adds external call sites; tests drive this now.
    fn box_uv(self) -> BoxUv {
        match self {
            // Sheep - base quadruped (64x32), head from sheep.tga
            MobPart::SheepHead => BoxUv { u0: 0,  v0: 0,  w: 6,  h: 6,  d: 8  },
            MobPart::SheepBody => BoxUv { u0: 28, v0: 8,  w: 8,  h: 16, d: 6  },

            // Zombie / humanoid (64x64)
            MobPart::ZombieHead     => BoxUv { u0: 0,  v0: 0,  w: 8, h: 8,  d: 8 },
            MobPart::ZombieBody     => BoxUv { u0: 16, v0: 16, w: 8, h: 12, d: 4 },
            MobPart::ZombieRightArm => BoxUv { u0: 40, v0: 16, w: 4, h: 12, d: 4 },
            MobPart::ZombieLeftArm  => BoxUv { u0: 32, v0: 48, w: 4, h: 12, d: 4 },
            MobPart::ZombieRightLeg => BoxUv { u0: 0,  v0: 16, w: 4, h: 12, d: 4 },
            MobPart::ZombieLeftLeg  => BoxUv { u0: 16, v0: 48, w: 4, h: 12, d: 4 },

            // Chicken (64x32)
            MobPart::ChickenHead     => BoxUv { u0: 0,  v0: 0, w: 4, h: 6, d: 3 },
            MobPart::ChickenBody     => BoxUv { u0: 0,  v0: 9, w: 6, h: 8, d: 6 },
            MobPart::ChickenLeftLeg  => BoxUv { u0: 26, v0: 0, w: 3, h: 5, d: 3 },
            MobPart::ChickenRightLeg => BoxUv { u0: 26, v0: 0, w: 3, h: 5, d: 3 },

            // Pig (64x64) - QuadrupedModel.createBodyMesh(legSize=6)
            MobPart::PigHead          => BoxUv { u0: 0,  v0: 0,  w: 8,  h: 8,  d: 8 },
            MobPart::PigBody          => BoxUv { u0: 28, v0: 8,  w: 10, h: 16, d: 8 },
            MobPart::PigLeftFrontLeg  => BoxUv { u0: 0,  v0: 16, w: 4,  h: 6,  d: 4 },
            MobPart::PigRightFrontLeg => BoxUv { u0: 0,  v0: 16, w: 4,  h: 6,  d: 4 },
            MobPart::PigLeftBackLeg   => BoxUv { u0: 0,  v0: 16, w: 4,  h: 6,  d: 4 },
            MobPart::PigRightBackLeg  => BoxUv { u0: 0,  v0: 16, w: 4,  h: 6,  d: 4 },

            // Cow (64x64) - QuadrupedModel.createBodyMesh(legSize=12)
            MobPart::CowHead          => BoxUv { u0: 0,  v0: 0,  w: 8,  h: 8,  d: 6  },
            MobPart::CowBody          => BoxUv { u0: 18, v0: 4,  w: 12, h: 18, d: 10 },
            MobPart::CowLeftFrontLeg  => BoxUv { u0: 0,  v0: 16, w: 4,  h: 12, d: 4  },
            MobPart::CowRightFrontLeg => BoxUv { u0: 0,  v0: 16, w: 4,  h: 12, d: 4  },
            MobPart::CowLeftBackLeg   => BoxUv { u0: 0,  v0: 16, w: 4,  h: 12, d: 4  },
            MobPart::CowRightBackLeg  => BoxUv { u0: 0,  v0: 16, w: 4,  h: 12, d: 4  },

            // Skeleton (64x32) - same humanoid layout, thinner limbs
            MobPart::SkeletonHead     => BoxUv { u0: 0,  v0: 0,  w: 8, h: 8,  d: 8 },
            MobPart::SkeletonBody     => BoxUv { u0: 16, v0: 16, w: 8, h: 12, d: 4 },
            MobPart::SkeletonRightArm => BoxUv { u0: 40, v0: 16, w: 2, h: 12, d: 2 },
            MobPart::SkeletonLeftArm  => BoxUv { u0: 40, v0: 16, w: 2, h: 12, d: 2 },
            MobPart::SkeletonRightLeg => BoxUv { u0: 0,  v0: 16, w: 2, h: 12, d: 2 },
            MobPart::SkeletonLeftLeg  => BoxUv { u0: 0,  v0: 16, w: 2, h: 12, d: 2 },

            // Spider (64x32)
            MobPart::SpiderHead  => BoxUv { u0: 32, v0: 4,  w: 8,  h: 8, d: 8  },
            MobPart::SpiderBody0 => BoxUv { u0: 0,  v0: 0,  w: 6,  h: 6, d: 6  },
            MobPart::SpiderBody1 => BoxUv { u0: 0,  v0: 12, w: 10, h: 8, d: 12 },
            MobPart::SpiderLeg   => BoxUv { u0: 18, v0: 0,  w: 16, h: 2, d: 2  },
        }
    }
}

// ---------------------------------------------------------------------------
// Mesh builder
// ---------------------------------------------------------------------------

/// Build a unit-cube mesh with per-face UVs matching the MC box-UV layout
/// for `part` on a texture of size `tex_w x tex_h`.
///
/// Returns `(vertices, indices)`:
/// - 24 vertices (4 per face x 6 faces)
/// - 36 indices  (6 per face)
///
/// The cube is centred on the origin with half-extent 0.5 on each axis.
/// Vertex order per face and index winding match `build_sheep_head_mesh`
/// in `main.rs` so the same renderer pipeline renders all mob parts.
// W19.2 adds the call site in main.rs; suppress the dead-code lint until then.
#[allow(dead_code)]
pub(crate) fn build_mob_part_mesh(
    part: MobPart,
    tex_w: u32,
    tex_h: u32,
) -> (Vec<MeshVertex>, Vec<u32>) {
    let BoxUv { u0, v0, w, h, d } = part.box_uv();
    build_box_uv_mesh(u0, v0, w, h, d, tex_w, tex_h)
}

/// Core builder: takes raw MC box-UV values and builds the cube mesh.
/// Extracted as a free function so it can be tested without constructing
/// a `MobPart`.
///
/// MC box-UV face layout (y-down texture, `+d` faces written as:
///
/// ```text
///   face     pixel rect in texture (pixel-coords, y-down)
///   top      (u0+d, v0)       .. (u0+d+w,   v0+d)
///   bottom   (u0+d+w, v0)     .. (u0+d+w+w, v0+d)
///   right    (u0,     v0+d)   .. (u0+d,     v0+d+h)
///   front    (u0+d,   v0+d)   .. (u0+d+w,   v0+d+h)
///   left     (u0+d+w, v0+d)   .. (u0+d+w+d, v0+d+h)
///   back     (u0+d+w+d, v0+d) .. (u0+d+w+d+w, v0+d+h)
/// ```
///
/// Render orientation: the box "front" face maps to the +X world face
/// (the mob's forward direction), same convention as `build_sheep_head_mesh`.
// W19.2 / test helpers drive this directly; suppress dead-code until then.
#[allow(dead_code)]
pub(crate) fn build_box_uv_mesh(
    u0: u32,
    v0: u32,
    w: u32,
    h: u32,
    d: u32,
    tex_w: u32,
    tex_h: u32,
) -> (Vec<MeshVertex>, Vec<u32>) {
    let tw = tex_w as f32;
    let th = tex_h as f32;

    // Normalise pixel coordinate to UV in [0, 1].
    let uv = |pu: u32, pv: u32| -> [f32; 2] { [pu as f32 / tw, pv as f32 / th] };

    // Pixel-coord helpers for the six face tiles.
    // top    row: v range [v0 .. v0+d]
    let top_u0 = u0 + d;
    let top_u1 = u0 + d + w;
    let top_v0 = v0;
    let top_v1 = v0 + d;

    // bottom row: same v range as top, shifted right by w
    let bot_u0 = u0 + d + w;
    let bot_u1 = u0 + d + w + w;
    // bot_v0/v1 = top_v0/top_v1  (same row)

    // side faces: v range [v0+d .. v0+d+h]
    let side_v0 = v0 + d;
    let side_v1 = v0 + d + h;

    // right face: u range [u0 .. u0+d]
    let right_u0 = u0;
    let right_u1 = u0 + d;

    // front face: u range [u0+d .. u0+d+w]
    let front_u0 = u0 + d;
    let front_u1 = u0 + d + w;

    // left face: u range [u0+d+w .. u0+d+w+d]
    let left_u0 = u0 + d + w;
    let left_u1 = u0 + d + w + d;

    // back face: u range [u0+d+w+d .. u0+d+w+d+w]
    let back_u0 = u0 + d + w + d;
    let back_u1 = u0 + d + w + d + w;

    let mut verts: Vec<MeshVertex> = Vec::with_capacity(24);
    let mut indices: Vec<u32> = Vec::with_capacity(36);

    // Emit one quad (4 verts + 6 indices). Vertex order:
    //   0: bottom-left, 1: bottom-right, 2: top-right, 3: top-left
    // when viewed from OUTSIDE the cube. UV convention (y-down texture):
    //   0: (u_left, v_bottom), 1: (u_right, v_bottom),
    //   2: (u_right, v_top),   3: (u_left, v_top)
    //
    // Index winding matches build_sheep_head_mesh:
    //   [base, base+2, base+1, base, base+3, base+2]
    let mut add_face = |corners: [[f32; 3]; 4],
                       normal: [f32; 3],
                       uv0: [f32; 2],
                       uv1: [f32; 2],
                       uv2: [f32; 2],
                       uv3: [f32; 2]| {
        let base = verts.len() as u32;
        let uvs = [uv0, uv1, uv2, uv3];
        for i in 0..4 {
            // W28.6: use builder API - direct struct init removed.
            verts.push(
                MeshVertex::new(corners[i], normal)
                    .with_uv(uvs[i])
                    .with_light(0.0, 1.0),
            );
        }
        indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    };

    let he = 0.5_f32; // half-extent of the unit cube

    // -Z face (MC "right" side of the head, looking from the front).
    // Pixel tile: right face [right_u0 .. right_u1] x [side_v0 .. side_v1]
    add_face(
        [[-he, -he, -he], [he, -he, -he], [he, he, -he], [-he, he, -he]],
        [0.0, 0.0, -1.0],
        uv(right_u0, side_v1),
        uv(right_u1, side_v1),
        uv(right_u1, side_v0),
        uv(right_u0, side_v0),
    );
    // +Z face (MC "left" side).
    // Pixel tile: left face [left_u0 .. left_u1] x [side_v0 .. side_v1]
    add_face(
        [[he, -he, he], [-he, -he, he], [-he, he, he], [he, he, he]],
        [0.0, 0.0, 1.0],
        uv(left_u0, side_v1),
        uv(left_u1, side_v1),
        uv(left_u1, side_v0),
        uv(left_u0, side_v0),
    );
    // +X face (MC "front" - the mob's face / forward direction).
    // Pixel tile: front face [front_u0 .. front_u1] x [side_v0 .. side_v1]
    add_face(
        [[he, -he, -he], [he, -he, he], [he, he, he], [he, he, -he]],
        [1.0, 0.0, 0.0],
        uv(front_u0, side_v1),
        uv(front_u1, side_v1),
        uv(front_u1, side_v0),
        uv(front_u0, side_v0),
    );
    // -X face (MC "back").
    // Pixel tile: back face [back_u0 .. back_u1] x [side_v0 .. side_v1]
    add_face(
        [[-he, -he, he], [-he, -he, -he], [-he, he, -he], [-he, he, he]],
        [-1.0, 0.0, 0.0],
        uv(back_u0, side_v1),
        uv(back_u1, side_v1),
        uv(back_u1, side_v0),
        uv(back_u0, side_v0),
    );
    // +Y face (top).
    // Pixel tile: top face [top_u0 .. top_u1] x [top_v0 .. top_v1]
    add_face(
        [[-he, he, -he], [he, he, -he], [he, he, he], [-he, he, he]],
        [0.0, 1.0, 0.0],
        uv(top_u0, top_v1),
        uv(top_u1, top_v1),
        uv(top_u1, top_v0),
        uv(top_u0, top_v0),
    );
    // -Y face (bottom).
    // Pixel tile: bottom face [bot_u0 .. bot_u1] x [top_v0 .. top_v1]
    add_face(
        [[-he, -he, he], [he, -he, he], [he, -he, -he], [-he, -he, -he]],
        [0.0, -1.0, 0.0],
        uv(bot_u0, top_v1),
        uv(bot_u1, top_v1),
        uv(bot_u1, top_v0),
        uv(bot_u0, top_v0),
    );

    (verts, indices)
}

// ---------------------------------------------------------------------------
// MobMeshes - per-kind GPU handle storage
// ---------------------------------------------------------------------------

/// Holds GPU mesh handles for every body part of every supported mob kind.
/// Each `Option<MeshHandle>` is `None` until the entity texture for that
/// kind is found on disk and uploaded during `on_start`. The render loop
/// checks `is_some()` and falls back to the tinted `player_cube` when
/// the handle is absent.
///
/// W19.2: populated by the asset-load block in `on_start`.
/// W19.3: consumed by the mob render dispatch.
#[derive(Default)]
pub(crate) struct MobMeshes {
    // ---- Zombie ------------------------------------------------------------
    pub(crate) zombie_head: Option<MeshHandle>,
    pub(crate) zombie_body: Option<MeshHandle>,
    pub(crate) zombie_right_arm: Option<MeshHandle>,
    pub(crate) zombie_left_arm: Option<MeshHandle>,
    pub(crate) zombie_right_leg: Option<MeshHandle>,
    pub(crate) zombie_left_leg: Option<MeshHandle>,

    // ---- Chicken -----------------------------------------------------------
    pub(crate) chicken_head: Option<MeshHandle>,
    pub(crate) chicken_body: Option<MeshHandle>,
    pub(crate) chicken_right_leg: Option<MeshHandle>,
    pub(crate) chicken_left_leg: Option<MeshHandle>,

    // ---- Pig ---------------------------------------------------------------
    pub(crate) pig_head: Option<MeshHandle>,
    pub(crate) pig_body: Option<MeshHandle>,
    pub(crate) pig_right_front_leg: Option<MeshHandle>,
    pub(crate) pig_left_front_leg: Option<MeshHandle>,
    pub(crate) pig_right_back_leg: Option<MeshHandle>,
    pub(crate) pig_left_back_leg: Option<MeshHandle>,

    // ---- Cow ---------------------------------------------------------------
    pub(crate) cow_head: Option<MeshHandle>,
    pub(crate) cow_body: Option<MeshHandle>,
    pub(crate) cow_right_front_leg: Option<MeshHandle>,
    pub(crate) cow_left_front_leg: Option<MeshHandle>,
    pub(crate) cow_right_back_leg: Option<MeshHandle>,
    pub(crate) cow_left_back_leg: Option<MeshHandle>,

    // ---- Skeleton ----------------------------------------------------------
    pub(crate) skeleton_head: Option<MeshHandle>,
    pub(crate) skeleton_body: Option<MeshHandle>,
    pub(crate) skeleton_right_arm: Option<MeshHandle>,
    pub(crate) skeleton_left_arm: Option<MeshHandle>,
    pub(crate) skeleton_right_leg: Option<MeshHandle>,
    pub(crate) skeleton_left_leg: Option<MeshHandle>,

    // ---- Spider ------------------------------------------------------------
    pub(crate) spider_head: Option<MeshHandle>,
    pub(crate) spider_body0: Option<MeshHandle>,
    pub(crate) spider_body1: Option<MeshHandle>,
    pub(crate) spider_leg: Option<MeshHandle>,
}

// W19.3 dispatch uses these helpers; suppress dead_code until then.
#[allow(dead_code)]
impl MobMeshes {
    /// True iff at least one part mesh was loaded for zombie.
    pub(crate) fn zombie_loaded(&self) -> bool {
        self.zombie_head.is_some()
    }
    /// True iff at least one part mesh was loaded for chicken.
    pub(crate) fn chicken_loaded(&self) -> bool {
        self.chicken_head.is_some()
    }
    /// True iff at least one part mesh was loaded for pig.
    pub(crate) fn pig_loaded(&self) -> bool {
        self.pig_head.is_some()
    }
    /// True iff at least one part mesh was loaded for cow.
    pub(crate) fn cow_loaded(&self) -> bool {
        self.cow_head.is_some()
    }
    /// True iff at least one part mesh was loaded for skeleton.
    pub(crate) fn skeleton_loaded(&self) -> bool {
        self.skeleton_head.is_some()
    }
    /// True iff at least one part mesh was loaded for spider.
    pub(crate) fn spider_loaded(&self) -> bool {
        self.spider_head.is_some()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- Generic mesh shape ---------------------------------------------------

    #[test]
    fn mesh_has_24_verts_and_36_indices() {
        for &part in all_parts() {
            let (v, i) = build_mob_part_mesh(part, 64, 32);
            assert_eq!(v.len(), 24, "{part:?}: expected 24 verts");
            assert_eq!(i.len(), 36, "{part:?}: expected 36 indices");
        }
    }

    #[test]
    fn all_uvs_in_0_1_range_64x32() {
        // Only test parts whose UV atlas fits within 64x32. Zombie and Cow
        // use 64x64 textures (second skin layer at rows 32-64); those are
        // covered by the 64x64 test below.
        let parts_64x32 = [
            MobPart::SheepHead,
            MobPart::SheepBody,
            MobPart::ChickenHead,
            MobPart::ChickenBody,
            MobPart::ChickenLeftLeg,
            MobPart::ChickenRightLeg,
            MobPart::SkeletonHead,
            MobPart::SkeletonBody,
            MobPart::SkeletonLeftLeg,
            MobPart::SkeletonRightLeg,
            MobPart::SkeletonLeftArm,
            MobPart::SkeletonRightArm,
            MobPart::SpiderHead,
            MobPart::SpiderBody0,
            MobPart::SpiderBody1,
            MobPart::SpiderLeg,
        ];
        for part in parts_64x32 {
            let (verts, _) = build_mob_part_mesh(part, 64, 32);
            for v in &verts {
                let uv = v.uv();
                assert!(
                    uv[0] >= 0.0 && uv[0] <= 1.0,
                    "{part:?}: u={} out of [0,1]",
                    uv[0]
                );
                assert!(
                    uv[1] >= 0.0 && uv[1] <= 1.0,
                    "{part:?}: v={} out of [0,1]",
                    uv[1]
                );
            }
        }
    }

    #[test]
    fn all_uvs_in_0_1_range_64x64() {
        // Some mobs use 64x64 textures (zombie, pig, cow, spider).
        for &part in all_parts() {
            let (verts, _) = build_mob_part_mesh(part, 64, 64);
            for v in &verts {
                let uv = v.uv();
                assert!(
                    uv[0] >= 0.0 && uv[0] <= 1.0,
                    "{part:?}: u={} out of [0,1] (64x64)",
                    uv[0]
                );
                assert!(
                    uv[1] >= 0.0 && uv[1] <= 1.0,
                    "{part:?}: v={} out of [0,1] (64x64)",
                    uv[1]
                );
            }
        }
    }

    #[test]
    fn index_values_within_vertex_range() {
        for &part in all_parts() {
            let (verts, indices) = build_mob_part_mesh(part, 64, 32);
            let n = verts.len() as u32;
            for &idx in &indices {
                assert!(idx < n, "{part:?}: index {idx} >= vert count {n}");
            }
        }
    }

    #[test]
    fn normals_are_unit_length() {
        for &part in all_parts() {
            let (verts, _) = build_mob_part_mesh(part, 64, 32);
            for v in &verts {
                let n = v.normal();
                let len = (n[0].powi(2) + n[1].powi(2) + n[2].powi(2)).sqrt();
                assert!(
                    (len - 1.0).abs() < 1e-5,
                    "{part:?}: normal length {len} != 1"
                );
            }
        }
    }

    #[test]
    fn light_and_anim_defaults() {
        for &part in all_parts() {
            let (verts, _) = build_mob_part_mesh(part, 64, 32);
            for v in &verts {
                let light = v.light();
                // block=0, sky=1 -> packed as [0, 255, 0, 0]; decoded
                // values may differ by 1/255 due to u8 rounding.
                assert!(light[0].abs() < 1e-4, "{part:?}: expected block=0, got {}", light[0]);
                assert!((light[1] - 1.0).abs() < 1e-4, "{part:?}: expected sky=1, got {}", light[1]);
                assert_eq!(v.anim_id, 0, "{part:?}: unexpected anim_id");
            }
        }
    }

    // -- UV layout spot-checks -----------------------------------------------

    #[test]
    fn zombie_head_front_face_uv_matches_mc_layout() {
        // ZombieHead: u0=0, v0=0, w=8, h=8, d=8 on a 64x64 texture.
        // Front face pixel region: [u0+d .. u0+d+w] x [v0+d .. v0+d+h]
        //   = [8 .. 16] x [8 .. 16]
        // Normalised: [0.125 .. 0.25] x [0.125 .. 0.25]
        let (verts, _) = build_mob_part_mesh(MobPart::ZombieHead, 64, 64);
        // +X face is vertices 8..12 (face index 2 * 4 = 8).
        let front_uvs: Vec<[f32; 2]> = verts[8..12].iter().map(|v| v.uv()).collect();
        // All u-coords should be in [8/64, 16/64] = [0.125, 0.25]
        for uv in &front_uvs {
            assert!(uv[0] >= 0.124 && uv[0] <= 0.251, "front u={}", uv[0]);
            assert!(uv[1] >= 0.124 && uv[1] <= 0.251, "front v={}", uv[1]);
        }
    }

    #[test]
    fn spider_leg_uv_within_64x32() {
        // SpiderLeg: u0=18, v0=0, w=16, h=2, d=2 on 64x32.
        // Rightmost pixel used: u0+d+w+d+w = 18+2+16+2+16 = 54 < 64 ok.
        // Bottom pixel: v0+d+h = 0+2+2 = 4 < 32 ok.
        let (verts, _) = build_mob_part_mesh(MobPart::SpiderLeg, 64, 32);
        for v in &verts {
            let uv = v.uv();
            assert!(uv[0] <= 1.0, "spider leg u out of bounds: {}", uv[0]);
            assert!(uv[1] <= 1.0, "spider leg v out of bounds: {}", uv[1]);
        }
    }

    #[test]
    fn build_box_uv_mesh_matches_build_mob_part_mesh() {
        // Smoke-test: raw call with SheepBody values should equal the
        // MobPart dispatch for the same part.
        let (v1, i1) = build_mob_part_mesh(MobPart::SheepBody, 64, 32);
        let BoxUv { u0, v0, w, h, d } = MobPart::SheepBody.box_uv();
        let (v2, i2) = build_box_uv_mesh(u0, v0, w, h, d, 64, 32);
        assert_eq!(i1, i2);
        assert_eq!(v1.len(), v2.len());
        for (a, b) in v1.iter().zip(v2.iter()) {
            // W28.6: compare decoded UVs (1/65535 rounding tolerance).
            let ua = a.uv();
            let ub = b.uv();
            assert!((ua[0] - ub[0]).abs() < 2e-5, "u mismatch: {} vs {}", ua[0], ub[0]);
            assert!((ua[1] - ub[1]).abs() < 2e-5, "v mismatch: {} vs {}", ua[1], ub[1]);
        }
    }

    // -- Helper: iterate all variants ----------------------------------------

    fn all_parts() -> &'static [MobPart] {
        &[
            MobPart::SheepHead,
            MobPart::SheepBody,
            MobPart::ZombieHead,
            MobPart::ZombieBody,
            MobPart::ZombieLeftLeg,
            MobPart::ZombieRightLeg,
            MobPart::ZombieLeftArm,
            MobPart::ZombieRightArm,
            MobPart::ChickenHead,
            MobPart::ChickenBody,
            MobPart::ChickenLeftLeg,
            MobPart::ChickenRightLeg,
            MobPart::PigHead,
            MobPart::PigBody,
            MobPart::PigLeftFrontLeg,
            MobPart::PigRightFrontLeg,
            MobPart::PigLeftBackLeg,
            MobPart::PigRightBackLeg,
            MobPart::CowHead,
            MobPart::CowBody,
            MobPart::CowLeftFrontLeg,
            MobPart::CowRightFrontLeg,
            MobPart::CowLeftBackLeg,
            MobPart::CowRightBackLeg,
            MobPart::SkeletonHead,
            MobPart::SkeletonBody,
            MobPart::SkeletonLeftLeg,
            MobPart::SkeletonRightLeg,
            MobPart::SkeletonLeftArm,
            MobPart::SkeletonRightArm,
            MobPart::SpiderHead,
            MobPart::SpiderBody0,
            MobPart::SpiderBody1,
            MobPart::SpiderLeg,
        ]
    }
}
