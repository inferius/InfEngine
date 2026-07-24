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
// BiomeVariant - W46.4
// ---------------------------------------------------------------------------

/// W46.4: MC 26.1.2 ships per-biome entity textures for chicken / pig /
/// cow / wolf / etc. (`chicken_temperate.png`, `chicken_cold.png`,
/// `chicken_warm.png`). Each entity carries a `Biome` property at spawn;
/// the client picks the texture file matching that biome. Mobs that
/// don't have biome variants (zombie, skeleton, spider) ignore this
/// and just load `<mob>.png`.
///
/// This enum mirrors MC's `BiomeKeys::TEMPERATE | COLD | WARM`. The
/// engine defaults all mobs to `Temperate` until a real biome system
/// lands; that matches MC's behaviour in plains / forest biomes
/// (the most common spawn locations) and produces the correct
/// canonical texture rather than a placeholder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(dead_code)] // Cold / Warm wired up once biome system lands.
pub(crate) enum BiomeVariant {
    /// Plains, forest, swamp, etc. - the MC default for most spawns.
    #[default]
    Temperate,
    /// Snowy plains, ice plains, frozen ocean.
    Cold,
    /// Desert, savanna, jungle, badlands.
    Warm,
}

impl BiomeVariant {
    /// Suffix MC appends to the base mob name for this variant
    /// (e.g. `_temperate` -> `chicken_temperate.png`).
    pub(crate) fn suffix(self) -> &'static str {
        match self {
            BiomeVariant::Temperate => "_temperate",
            BiomeVariant::Cold => "_cold",
            BiomeVariant::Warm => "_warm",
        }
    }
}

/// True when the given MC entity name has per-biome texture files.
/// MC 26.x split these out; older mobs (zombie, skeleton, spider,
/// sheep, etc.) still ship a single `<name>.png`.
pub(crate) fn entity_has_biome_variants(mob: &str) -> bool {
    matches!(mob, "chicken" | "pig" | "cow")
}

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
    // Source: SheepModel.java -> QuadrupedModel.createBodyMesh(legSize=12)
    // head texOffs(0,0)   addBox size [6,6,8]
    // body texOffs(28,8)  addBox size [8,16,6] (rotated 90 in MC)
    // leg  texOffs(0,16)  addBox size [4,12,4]  (all 4 legs share UV)
    SheepHead,
    SheepBody,
    SheepLeftFrontLeg,
    SheepRightFrontLeg,
    SheepLeftBackLeg,
    SheepRightBackLeg,

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
            // W55.3: all 4 sheep legs share the same MC leg-UV tile.
            MobPart::SheepLeftFrontLeg => BoxUv { u0: 0, v0: 16, w: 4, h: 12, d: 4 },
            MobPart::SheepRightFrontLeg => BoxUv { u0: 0, v0: 16, w: 4, h: 12, d: 4 },
            MobPart::SheepLeftBackLeg => BoxUv { u0: 0, v0: 16, w: 4, h: 12, d: 4 },
            MobPart::SheepRightBackLeg => BoxUv { u0: 0, v0: 16, w: 4, h: 12, d: 4 },

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
    // ---- Sheep -------------------------------------------------------------
    pub(crate) sheep_head: Option<MeshHandle>,
    pub(crate) sheep_body: Option<MeshHandle>,
    pub(crate) sheep_right_front_leg: Option<MeshHandle>,
    pub(crate) sheep_left_front_leg: Option<MeshHandle>,
    pub(crate) sheep_right_back_leg: Option<MeshHandle>,
    pub(crate) sheep_left_back_leg: Option<MeshHandle>,
    /// W92 - sheep baked from universal MC `SheepModel` data.
    pub(crate) sheep_model: Option<crate::entity_model::BakedModel>,
    /// W139 - the `SheepFurModel` wool overlay baked from
    /// `sheep/sheep_wool.png`. Drawn over `sheep_model` at the same
    /// pose; `None` when the pack lacks the wool sheet (the sheep
    /// then shows the bare skin like before).
    pub(crate) sheep_fur_model: Option<crate::entity_model::BakedModel>,

    // ---- Zombie ------------------------------------------------------------
    pub(crate) zombie_head: Option<MeshHandle>,
    pub(crate) zombie_body: Option<MeshHandle>,
    pub(crate) zombie_right_arm: Option<MeshHandle>,
    pub(crate) zombie_left_arm: Option<MeshHandle>,
    pub(crate) zombie_right_leg: Option<MeshHandle>,
    pub(crate) zombie_left_leg: Option<MeshHandle>,
    /// W92 - zombie baked from universal MC `ZombieModel` data.
    pub(crate) zombie_model: Option<crate::entity_model::BakedModel>,
    /// W300 - zombified piglin (universal `AdultPiglinModel` data).
    pub(crate) zombified_piglin_model: Option<crate::entity_model::BakedModel>,
    /// W302 - villager (universal `VillagerModel` data).
    pub(crate) villager_model: Option<crate::entity_model::BakedModel>,
    /// W315 - cat (universal `AdultFelineModel` data).
    pub(crate) cat_model: Option<crate::entity_model::BakedModel>,
    /// W322 - iron golem (universal `IronGolemModel` data).
    pub(crate) iron_golem_model: Option<crate::entity_model::BakedModel>,
    /// W322 - horse (universal `AbstractEquineModel` data).
    pub(crate) horse_model: Option<crate::entity_model::BakedModel>,
    /// W327 - ghast (universal `GhastModel` data, scale 4.5).
    pub(crate) ghast_model: Option<crate::entity_model::BakedModel>,
    /// W330 - zombie villager (villager mesh, zombie skin).
    pub(crate) zombie_villager_model: Option<crate::entity_model::BakedModel>,
    /// W343: creeper (CreeperModel verbatim).
    pub(crate) creeper_model: Option<crate::entity_model::BakedModel>,
    /// W344: donkey (DonkeyModel - equine + panniers, scale 0.87).
    pub(crate) donkey_model: Option<crate::entity_model::BakedModel>,
    /// W375: bee (AdultBeeModel verbatim).
    pub(crate) bee_model: Option<crate::entity_model::BakedModel>,
    /// W376: witch (villager mesh, entity/witch.png sheet - the
    /// vanilla WitchModel is villager-derived).
    pub(crate) witch_model: Option<crate::entity_model::BakedModel>,
    /// W376: slime (magma-cube mesh, entity/slime/slime.png).
    pub(crate) slime_model: Option<crate::entity_model::BakedModel>,
    /// W377: zombie variants (zombie mesh, variant sheets).
    pub(crate) drowned_model: Option<crate::entity_model::BakedModel>,
    pub(crate) husk_model: Option<crate::entity_model::BakedModel>,
    /// W377: stray (skeleton mesh, frost sheet).
    pub(crate) stray_model: Option<crate::entity_model::BakedModel>,
    /// W378: illager family (one mesh, three sheets).
    pub(crate) pillager_model: Option<crate::entity_model::BakedModel>,
    pub(crate) vindicator_model: Option<crate::entity_model::BakedModel>,
    pub(crate) evoker_model: Option<crate::entity_model::BakedModel>,
    /// W327 - magma cube (universal `MagmaCubeModel`, scale 2.0).
    pub(crate) magma_cube_model: Option<crate::entity_model::BakedModel>,

    // ---- Chicken -----------------------------------------------------------
    pub(crate) chicken_head: Option<MeshHandle>,
    pub(crate) chicken_body: Option<MeshHandle>,
    pub(crate) chicken_right_leg: Option<MeshHandle>,
    pub(crate) chicken_left_leg: Option<MeshHandle>,
    /// W92 - chicken baked from universal MC `ChickenModel` data
    /// (incl. beak, wattle, wings).
    pub(crate) chicken_model: Option<crate::entity_model::BakedModel>,
    /// W158 - wolf baked from universal MC `WolfModel` data.
    pub(crate) wolf_model: Option<crate::entity_model::BakedModel>,
    /// W161 - polar bear baked from universal MC `PolarBearModel`.
    pub(crate) polar_bear_model: Option<crate::entity_model::BakedModel>,
    /// W162 - rabbit baked from universal MC `AdultRabbitModel`
    /// (first nested bone hierarchy).
    pub(crate) rabbit_model: Option<crate::entity_model::BakedModel>,

    // ---- Pig ---------------------------------------------------------------
    pub(crate) pig_head: Option<MeshHandle>,
    pub(crate) pig_body: Option<MeshHandle>,
    pub(crate) pig_right_front_leg: Option<MeshHandle>,
    pub(crate) pig_left_front_leg: Option<MeshHandle>,
    pub(crate) pig_right_back_leg: Option<MeshHandle>,
    pub(crate) pig_left_back_leg: Option<MeshHandle>,
    /// W92 - pig baked from universal MC `PigModel` data (incl. snout).
    pub(crate) pig_model: Option<crate::entity_model::BakedModel>,

    // ---- Cow ---------------------------------------------------------------
    pub(crate) cow_head: Option<MeshHandle>,
    pub(crate) cow_body: Option<MeshHandle>,
    pub(crate) cow_right_front_leg: Option<MeshHandle>,
    pub(crate) cow_left_front_leg: Option<MeshHandle>,
    pub(crate) cow_right_back_leg: Option<MeshHandle>,
    pub(crate) cow_left_back_leg: Option<MeshHandle>,
    /// W91 - the cow baked from universal MC `CowModel` data (horns +
    /// body xRot). When present, render uses this instead of the legacy
    /// six magic-number part transforms above.
    pub(crate) cow_model: Option<crate::entity_model::BakedModel>,

    // ---- Skeleton ----------------------------------------------------------
    pub(crate) skeleton_head: Option<MeshHandle>,
    pub(crate) skeleton_body: Option<MeshHandle>,
    pub(crate) skeleton_right_arm: Option<MeshHandle>,
    pub(crate) skeleton_left_arm: Option<MeshHandle>,
    pub(crate) skeleton_right_leg: Option<MeshHandle>,
    pub(crate) skeleton_left_leg: Option<MeshHandle>,
    /// W92 - skeleton baked from universal MC `SkeletonModel` data.
    pub(crate) skeleton_model: Option<crate::entity_model::BakedModel>,

    // ---- Spider ------------------------------------------------------------
    pub(crate) spider_head: Option<MeshHandle>,
    pub(crate) spider_body0: Option<MeshHandle>,
    pub(crate) spider_body1: Option<MeshHandle>,
    pub(crate) spider_leg: Option<MeshHandle>,
    /// W92 - spider baked from universal MC `SpiderModel` data (8 splayed
    /// legs + two body segments).
    pub(crate) spider_model: Option<crate::entity_model::BakedModel>,
}

// W19.3 dispatch uses these helpers; suppress dead_code until then.
#[allow(dead_code)]
impl MobMeshes {
    /// True iff at least one part mesh was loaded for sheep.
    pub(crate) fn sheep_loaded(&self) -> bool {
        self.sheep_head.is_some()
    }
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


// ===========================================================================
// W47.1: relocated from main.rs
//
// build_sheep_head_mesh, load_non_sheep_mob_meshes, tick_chicken_feather_lay
// used to live inline in main.rs (which had ballooned to 17k+ lines).
// They are pure functions over public renderer types + this module's own
// MobMeshes / MobPart, so they belong here. Call sites in main.rs +
// debug_tests.rs now go through `crate::mob_meshes::<fn>`.
// ===========================================================================

use infengine_render::Renderer;
use tracing::{info, warn};

/// Build a 1x1x1 cube mesh with per-face UVs targeting the MC
/// sheep head texture layout. The face strip lives in the top-
/// left of a 64-wide texture; each 8x8 tile maps to one face of
/// an 8x8x8 cube unwrap:
///
/// ```text
///                u
///        +---+---+---+
///        | T | B |   |     0..8: -  8..16: top  16..24: bottom
///   v    +---+---+---+---+
///        | R | F | L | K |   right, front, left, back
///        +---+---+---+---+
/// ```
///
/// `tex_w` / `tex_h` are the source texture dimensions; UVs are
/// normalised against them so the same layout works for Java
/// (64x32) and Bedrock (64x64) sheep textures.
pub fn build_sheep_head_mesh(tex_w: u32, tex_h: u32) -> (Vec<MeshVertex>, Vec<u32>) {
    let tw = tex_w as f32;
    let th = tex_h as f32;
    // Helper: convert pixel coordinates to normalised UV.
    let uv = |u: u32, v: u32| -> [f32; 2] { [u as f32 / tw, v as f32 / th] };
    let mut verts: Vec<MeshVertex> = Vec::with_capacity(24);
    let mut indices: Vec<u32> = Vec::with_capacity(36);
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
            // No block light, full sky; anim_id = 0 (static tile).
            verts.push(
                MeshVertex::new(corners[i], normal)
                    .with_uv(uvs[i])
                    .with_light(0.0, 1.0),
            );
        }
        // Engine cube pipeline uses CCW front-face but the
        // `cube_geometry` reference winds quads as
        // `[0, 2, 1, 0, 3, 2]` when corners are listed
        // bottom-left -> bottom-right -> top-right -> top-left
        // viewed from outside. Matching that here so back-face
        // culling hides the interior of the head, not the outside.
        indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    };
    // Cube corners centred on the origin, edge length 1. Vertex
    // order per face is bottom-left, bottom-right, top-right,
    // top-left when viewed from outside the cube. UV (0,0) is
    // top-left of the texture, (1,1) is bottom-right - so the
    // matching corner UV order is `(u0,v1)`, `(u1,v1)`, `(u1,v0)`,
    // `(u0,v0)`.
    let h = 0.5_f32;
    // Bedrock sheep.geo.json: head bone has cube size [6, 6, 8] at
    // texture origin (0, 0). That's MC box-uv layout, NOT a plain
    // 8x8 face strip. The unwrap (with `d` = depth, `w` = width,
    // `h` = height):
    //
    //   ```
    //   (d, 0)         top  size w x d  -> (8, 0)..(14, 8)
    //   (d+w, 0)       bottom size w x d -> (14, 0)..(20, 8)
    //   (0, d)         right size d x h -> (0, 8)..(8, 14)
    //   (d, d)         front size w x h -> (8, 8)..(14, 14)
    //   (d+w, d)       left  size d x h -> (14, 8)..(22, 14)
    //   (d+w+d, d)     back  size w x h -> (22, 8)..(28, 14)
    //   ```
    //
    // Our render orientation has the sheep facing +X (the head's
    // depth axis runs along world X). We map MC's "front" (face)
    // tile onto our +X face so the eyes / mouth follow the
    // sheep's movement direction.
    //
    // -Z face: head's right side (MC east). Tile (0, 8)..(8, 14).
    add_face(
        [[-h, -h, -h], [h, -h, -h], [h, h, -h], [-h, h, -h]],
        [0.0, 0.0, -1.0],
        uv(0, 14),
        uv(8, 14),
        uv(8, 8),
        uv(0, 8),
    );
    // +Z face: head's left side (MC west). Tile (14, 8)..(22, 14).
    add_face(
        [[h, -h, h], [-h, -h, h], [-h, h, h], [h, h, h]],
        [0.0, 0.0, 1.0],
        uv(14, 14),
        uv(22, 14),
        uv(22, 8),
        uv(14, 8),
    );
    // +X face: the sheep's FACE. MC "front" tile (8, 8)..(14, 14).
    add_face(
        [[h, -h, -h], [h, -h, h], [h, h, h], [h, h, -h]],
        [1.0, 0.0, 0.0],
        uv(8, 14),
        uv(14, 14),
        uv(14, 8),
        uv(8, 8),
    );
    // -X face: back of head. MC "back" tile (22, 8)..(28, 14).
    add_face(
        [[-h, -h, h], [-h, -h, -h], [-h, h, -h], [-h, h, h]],
        [-1.0, 0.0, 0.0],
        uv(22, 14),
        uv(28, 14),
        uv(28, 8),
        uv(22, 8),
    );
    // Top face. MC "top" tile (8, 0)..(14, 8).
    add_face(
        [[-h, h, -h], [h, h, -h], [h, h, h], [-h, h, h]],
        [0.0, 1.0, 0.0],
        uv(8, 8),
        uv(14, 8),
        uv(14, 0),
        uv(8, 0),
    );
    // Bottom face. MC "bottom" tile (14, 0)..(20, 8).
    add_face(
        [[-h, -h, h], [h, -h, h], [h, -h, -h], [-h, -h, -h]],
        [0.0, -1.0, 0.0],
        uv(14, 8),
        uv(20, 8),
        uv(20, 0),
        uv(14, 0),
    );
    (verts, indices)
}

/// W19.2 - Discover and upload entity texture meshes for the 6 non-sheep
/// mob kinds (Zombie, Chicken, Pig, Cow, Skeleton, Spider).
///
/// For each kind:
/// 1. Search `<texture_dir>/../entity/<mob>/<mob>.tga` and `.png`.
///    (Same fallback pattern as the sheep head loader above.)
/// 2. Load via `image::open`, force alpha = 255.
/// 3. Build per-part cube meshes via `mob_meshes::build_mob_part_mesh`.
/// 4. Upload + bind via `r.create_mesh` / `r.make_mesh_albedo` /
///    `r.bind_mesh_albedo_arc`.
///
/// Returns a fully populated `MobMeshes` (all `None` for any mob whose
/// texture file wasn't found on disk).
pub fn load_non_sheep_mob_meshes(
    texture_dir: &std::path::Path,
    mc_entity_dir: Option<&std::path::Path>,
    r: &mut Renderer,
) -> MobMeshes {

    // W46.9: search BOTH the MC pack entity dir (canonical source - MC 26.1+
    // ships per-mob `<mob>/<mob>.png` only inside the cached pack) AND the
    // local engine assets dir (holdover from pre-MC waves, hosts the bundled
    // sheep .tga). The MC pack root is preferred so 26.x biome variants win.
    let local_entity_root = texture_dir
        .parent()
        .unwrap_or(texture_dir)
        .join("entity");
    let entity_roots: Vec<std::path::PathBuf> = match mc_entity_dir {
        Some(mc) => vec![mc.to_path_buf(), local_entity_root.clone()],
        None => vec![local_entity_root.clone()],
    };

    /// Try .tga then .png in the canonical MC entity sub-directory.
    /// Returns the image bytes as an RGBA `image::RgbaImage`, or `None`
    /// if neither file exists or the image can't be decoded.
    ///
    /// W46.4: MC 26.1.2 split chicken / pig / cow textures into per-biome
    /// files (`chicken_temperate.png`, `chicken_cold.png`, `chicken_warm.png`).
    /// For mobs in `entity_has_biome_variants`, the loader resolves the
    /// canonical `<mob>{biome.suffix}.png` first (proper MC behaviour with
    /// the engine-wide default biome = Temperate, NOT a "load any file we
    /// can find" fallback). Mobs without biome variants still use the bare
    /// `<mob>.png` path.
    ///
    /// W31.2: logs a warn when no candidate is found so the user knows
    /// they need a resource pack for that mob's texture.
    fn try_open_entity(
        entity_roots: &[std::path::PathBuf],
        mob: &str,
        biome: BiomeVariant,
    ) -> Option<image::RgbaImage> {
        // Per-biome filename (e.g. "chicken_temperate") - the canonical
        // MC 26.x name for mobs with biome variants. For non-variant
        // mobs this is unused.
        let biome_name = format!("{mob}{}", biome.suffix());
        let has_biome = entity_has_biome_variants(mob);

        // W46.9: build candidate list spanning every search root. Within
        // each root the biome-variant names come first so MC 26.x packs
        // win even when an older bare-name file is also present.
        let mut candidates: Vec<std::path::PathBuf> = Vec::new();
        for entity_root in entity_roots {
            let mob_dir = entity_root.join(mob);
            if has_biome {
                candidates.push(mob_dir.join(format!("{biome_name}.tga")));
                candidates.push(mob_dir.join(format!("{biome_name}.png")));
                candidates.push(entity_root.join(format!("{biome_name}.tga")));
                candidates.push(entity_root.join(format!("{biome_name}.png")));
            }
            // Bare-name candidates (works for zombie / skeleton / spider /
            // pre-26.x packs).
            candidates.push(mob_dir.join(format!("{mob}.tga")));
            candidates.push(mob_dir.join(format!("{mob}.png")));
            // Bedrock packs often flatten the entity dir:
            candidates.push(entity_root.join(format!("{mob}.tga")));
            candidates.push(entity_root.join(format!("{mob}.png")));
            // Some Java packs place the texture one level up from entity/:
            if let Some(parent) = entity_root.parent() {
                candidates.push(parent.join(format!("{mob}.tga")));
                candidates.push(parent.join(format!("{mob}.png")));
            }
        }

        let path = match candidates.iter().find(|p| p.is_file()) {
            Some(p) => p,
            None => {
                warn!(
                    mob = mob,
                    biome = ?biome,
                    searched = ?candidates.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
                    "entity texture not found - mob will be untextured (install a resource pack)"
                );
                return None;
            }
        };
        let mut img = image::open(path).ok()?.to_rgba8();
        // W141: flatten alpha for legacy .tga exports ONLY - those carry
        // soft pre-multiplied edges that would punch halo holes through
        // the cutout discard. MC .png entity sheets are hard-edged cutout
        // art whose transparent texels are MEANT to vanish (thin chicken
        // legs, skeleton rib gaps); the previous unconditional flatten
        // turned them into opaque black boxes (rgb 0 + forced alpha 255
        // survives the shader's `alpha < 0.5` discard).
        let is_tga = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("tga"));
        if is_tga {
            for px in img.chunks_exact_mut(4) {
                px[3] = 255;
            }
        }
        Some(img)
    }

    // W46.4: engine-wide default biome until a real biome system lands.
    // MC's plains / forest biomes resolve to Temperate so this matches the
    // common-case appearance for spawned mobs.
    let biome = BiomeVariant::default();

    /// Upload a single mesh part and return its handle.
    fn upload_part(
        r: &mut Renderer,
        label: &str,
        part: MobPart,
        img: &image::RgbaImage,
    ) -> MeshHandle {
        let (verts, indices) = build_mob_part_mesh(part, img.width(), img.height());
        let albedo = r.make_mesh_albedo(img.width(), img.height(), img);
        let mesh = r.create_mesh(label, &verts, &indices);
        r.bind_mesh_albedo_arc(mesh, albedo);
        mesh
    }

    let mut mm = MobMeshes::default();

    // ---- Sheep (64x32 base + wool overlay) ---------------------------------
    // W55.3: sheep gets the full QuadrupedModel treatment (body +
    // head + 4 legs) instead of the legacy 2-cube placeholder.
    if let Some(img) = try_open_entity(&entity_roots, "sheep", biome) {
        let (w, h) = (img.width(), img.height());
        mm.sheep_head          = Some(upload_part(r, "voxel_demo_mp::sheep_head",  MobPart::SheepHead,          &img));
        mm.sheep_body          = Some(upload_part(r, "voxel_demo_mp::sheep_body",  MobPart::SheepBody,          &img));
        mm.sheep_right_front_leg = Some(upload_part(r, "voxel_demo_mp::sheep_rf_leg", MobPart::SheepRightFrontLeg, &img));
        mm.sheep_left_front_leg  = Some(upload_part(r, "voxel_demo_mp::sheep_lf_leg", MobPart::SheepLeftFrontLeg,  &img));
        mm.sheep_right_back_leg  = Some(upload_part(r, "voxel_demo_mp::sheep_rb_leg", MobPart::SheepRightBackLeg,  &img));
        mm.sheep_left_back_leg   = Some(upload_part(r, "voxel_demo_mp::sheep_lb_leg", MobPart::SheepLeftBackLeg,   &img));
        // W92 - universal MC SheepModel. Render prefers this.
        mm.sheep_model = Some(crate::entity_model::bake(r, &crate::entity_model::sheep::MODEL, &img));
        info!(texture = format!("{w}x{h}"), "sheep entity texture bound (universal model + 6 legacy parts)");
        // W139 - the SheepFurModel wool overlay. MC 26.x ships it
        // at entity/sheep/sheep_wool.png (older packs name it
        // sheep_fur.png); without it the sheep showed bare skin.
        let wool = entity_roots
            .iter()
            .flat_map(|root| {
                [
                    root.join("sheep").join("sheep_wool.png"),
                    root.join("sheep").join("sheep_fur.png"),
                ]
            })
            .find(|p| p.is_file())
            .and_then(|p| image::open(&p).ok().map(|i| i.to_rgba8()));
        if let Some(wool_img) = wool {
            let mut fur = crate::entity_model::bake(
                r,
                &crate::entity_model::sheep::FUR_MODEL,
                &wool_img,
            );
            // The wool "socks" stop above the hooves, so the fur
            // model's own ground offset sits higher than the skin's.
            // Pin it to the SKIN offset so both layers share one
            // ground plane (otherwise the wool floats ~5 px high).
            if let Some(skin) = &mm.sheep_model {
                fur.ground_offset = skin.ground_offset;
            }
            mm.sheep_fur_model = Some(fur);
            info!("sheep wool overlay bound (SheepFurModel)");
        }
    }

    // ---- Zombie (64x64) ----------------------------------------------------
    if let Some(img) = try_open_entity(&entity_roots,"zombie", biome) {
        let (w, h) = (img.width(), img.height());
        mm.zombie_head      = Some(upload_part(r, "voxel_demo_mp::zombie_head",      MobPart::ZombieHead,     &img));
        mm.zombie_body      = Some(upload_part(r, "voxel_demo_mp::zombie_body",      MobPart::ZombieBody,     &img));
        mm.zombie_right_arm = Some(upload_part(r, "voxel_demo_mp::zombie_right_arm", MobPart::ZombieRightArm, &img));
        mm.zombie_left_arm  = Some(upload_part(r, "voxel_demo_mp::zombie_left_arm",  MobPart::ZombieLeftArm,  &img));
        mm.zombie_right_leg = Some(upload_part(r, "voxel_demo_mp::zombie_right_leg", MobPart::ZombieRightLeg, &img));
        mm.zombie_left_leg  = Some(upload_part(r, "voxel_demo_mp::zombie_left_leg",  MobPart::ZombieLeftLeg,  &img));
        // W92 - universal MC ZombieModel. Render prefers this.
        mm.zombie_model = Some(crate::entity_model::bake(r, &crate::entity_model::zombie::MODEL, &img));
        info!(texture = format!("{w}x{h}"), "zombie entity texture bound (universal model + 6 legacy parts)");
    }

    // ---- Zombified piglin (64x64) ------------------------------------------
    // W300: MC ships it under the piglin subdir
    // (entity/piglin/zombified_piglin.png), so try_open_entity's
    // bare-name candidates would miss it - direct open like the
    // polar bear's bear/polarbear.png.
    for root in &entity_roots {
        let p = root.join("piglin").join("zombified_piglin.png");
        if let Ok(img) = image::open(&p).map(|i| i.to_rgba8()) {
            let (w, h) = (img.width(), img.height());
            mm.zombified_piglin_model = Some(crate::entity_model::bake(
                r,
                &crate::entity_model::piglin::MODEL,
                &img,
            ));
            info!(
                texture = format!("{w}x{h}"),
                "zombified piglin entity texture bound (universal model)"
            );
            break;
        }
    }

    // ---- Villager (64x64, entity/villager subdir) --------------------------
    // W302: base skin only; the type/profession overlay layers are a
    // follow-up (they composite three textures in MC).
    for root in &entity_roots {
        let p = root.join("villager").join("villager.png");
        if let Ok(img) = image::open(&p).map(|i| i.to_rgba8()) {
            let (w, h) = (img.width(), img.height());
            mm.villager_model = Some(crate::entity_model::bake(
                r,
                &crate::entity_model::villager::MODEL,
                &img,
            ));
            info!(
                texture = format!("{w}x{h}"),
                "villager entity texture bound (universal model)"
            );
            break;
        }
    }

    // ---- Cat (64x32, entity/cat subdir) ------------------------------------
    // W315: tabby default (MC CatSpawner rolls variants; per-anchor
    // variant selection is a follow-up).
    for root in &entity_roots {
        let p = root.join("cat").join("cat_tabby.png");
        if let Ok(img) = image::open(&p).map(|i| i.to_rgba8()) {
            let (w, h) = (img.width(), img.height());
            mm.cat_model = Some(crate::entity_model::bake(
                r,
                &crate::entity_model::cat::MODEL,
                &img,
            ));
            info!(
                texture = format!("{w}x{h}"),
                "cat entity texture bound (universal model)"
            );
            break;
        }
    }

    // ---- Iron golem (128x128, entity/iron_golem subdir) --------------------
    for root in &entity_roots {
        let p = root.join("iron_golem").join("iron_golem.png");
        if let Ok(img) = image::open(&p).map(|i| i.to_rgba8()) {
            let (w, h) = (img.width(), img.height());
            mm.iron_golem_model = Some(crate::entity_model::bake(
                r,
                &crate::entity_model::iron_golem::MODEL,
                &img,
            ));
            info!(
                texture = format!("{w}x{h}"),
                "iron golem entity texture bound (universal model)"
            );
            break;
        }
    }

    // ---- Horse (64x64, entity/horse subdir) --------------------------------
    // W322: brown default (variant rolls are a follow-up).
    for root in &entity_roots {
        let p = root.join("horse").join("horse_brown.png");
        if let Ok(img) = image::open(&p).map(|i| i.to_rgba8()) {
            let (w, h) = (img.width(), img.height());
            mm.horse_model = Some(crate::entity_model::bake(
                r,
                &crate::entity_model::horse::MODEL,
                &img,
            ));
            info!(
                texture = format!("{w}x{h}"),
                "horse entity texture bound (universal model)"
            );
            break;
        }
    }

    // ---- Zombie villager (villager mesh + zombie_villager skin) ------------
    for root in &entity_roots {
        let p = root.join("zombie_villager").join("zombie_villager.png");
        if let Ok(img) = image::open(&p).map(|i| i.to_rgba8()) {
            let (w, h) = (img.width(), img.height());
            mm.zombie_villager_model = Some(crate::entity_model::bake(
                r,
                &crate::entity_model::villager::MODEL,
                &img,
            ));
            info!(
                texture = format!("{w}x{h}"),
                "zombie villager entity texture bound (villager mesh)"
            );
            break;
        }
    }

    // ---- Ghast (64x32, entity/ghast subdir) --------------------------------
    for root in &entity_roots {
        let p = root.join("ghast").join("ghast.png");
        if let Ok(img) = image::open(&p).map(|i| i.to_rgba8()) {
            let (w, h) = (img.width(), img.height());
            mm.ghast_model = Some(crate::entity_model::bake(
                r,
                &crate::entity_model::ghast::MODEL,
                &img,
            ));
            info!(
                texture = format!("{w}x{h}"),
                "ghast entity texture bound (universal model)"
            );
            break;
        }
    }

    // ---- Donkey (64x64, entity/horse subdir) -------------------------------
    for root in &entity_roots {
        let p = root.join("horse").join("donkey.png");
        if let Ok(img) = image::open(&p).map(|i| i.to_rgba8()) {
            let (w, h) = (img.width(), img.height());
            mm.donkey_model = Some(crate::entity_model::bake(
                r,
                &crate::entity_model::donkey::MODEL,
                &img,
            ));
            info!(
                texture = format!("{w}x{h}"),
                "donkey entity texture bound (universal model)"
            );
            break;
        }
    }

    // ---- Creeper (64x32) ---------------------------------------------------
    for root in &entity_roots {
        let p = root.join("creeper").join("creeper.png");
        if let Ok(img) = image::open(&p).map(|i| i.to_rgba8()) {
            let (w, h) = (img.width(), img.height());
            mm.creeper_model = Some(crate::entity_model::bake(
                r,
                &crate::entity_model::creeper::MODEL,
                &img,
            ));
            info!(
                texture = format!("{w}x{h}"),
                "creeper entity texture bound (universal model)"
            );
            break;
        }
    }

    // ---- Bee (64x64) -------------------------------------------------------
    for root in &entity_roots {
        let p = root.join("bee").join("bee.png");
        if let Ok(img) = image::open(&p).map(|i| i.to_rgba8()) {
            let (w, h) = (img.width(), img.height());
            mm.bee_model = Some(crate::entity_model::bake(
                r,
                &crate::entity_model::bee::MODEL,
                &img,
            ));
            info!(
                texture = format!("{w}x{h}"),
                "bee entity texture bound (universal model)"
            );
            break;
        }
    }

    // ---- Zombie/skeleton variant reskins (W377) ----------------------------
    for (slot, sub, file, model_kind) in [
        (0usize, "zombie", "drowned.png", 0u8),
        (1, "zombie", "husk.png", 0),
        (2, "skeleton", "stray.png", 1),
    ] {
        for root in &entity_roots {
            let p = root.join(sub).join(file);
            if let Ok(img) = image::open(&p).map(|i| i.to_rgba8()) {
                let baked = if model_kind == 0 {
                    crate::entity_model::bake(
                        r,
                        &crate::entity_model::zombie::MODEL,
                        &img,
                    )
                } else {
                    crate::entity_model::bake(
                        r,
                        &crate::entity_model::skeleton::MODEL,
                        &img,
                    )
                };
                match slot {
                    0 => mm.drowned_model = Some(baked),
                    1 => mm.husk_model = Some(baked),
                    _ => mm.stray_model = Some(baked),
                }
                info!(file, "variant entity texture bound (reskin)");
                break;
            }
        }
    }

    // ---- Illager family (W378: one IllagerModel, three sheets) -------------
    for (slot, file) in
        [(0usize, "pillager.png"), (1, "vindicator.png"), (2, "evoker.png")]
    {
        for root in &entity_roots {
            let p = root.join("illager").join(file);
            if let Ok(img) = image::open(&p).map(|i| i.to_rgba8()) {
                let baked = crate::entity_model::bake(
                    r,
                    &crate::entity_model::illager::MODEL,
                    &img,
                );
                match slot {
                    0 => mm.pillager_model = Some(baked),
                    1 => mm.vindicator_model = Some(baked),
                    _ => mm.evoker_model = Some(baked),
                }
                info!(file, "illager entity texture bound");
                break;
            }
        }
    }

    // ---- Witch (villager mesh, flat entity/witch.png) ----------------------
    for root in &entity_roots {
        let p = root.join("witch.png");
        if let Ok(img) = image::open(&p).map(|i| i.to_rgba8()) {
            mm.witch_model = Some(crate::entity_model::bake(
                r,
                &crate::entity_model::villager::MODEL,
                &img,
            ));
            info!("witch entity texture bound (villager mesh)");
            break;
        }
    }

    // ---- Slime (magma-cube mesh, entity/slime/slime.png) -------------------
    for root in &entity_roots {
        let p = root.join("slime").join("slime.png");
        if let Ok(img) = image::open(&p).map(|i| i.to_rgba8()) {
            mm.slime_model = Some(crate::entity_model::bake(
                r,
                &crate::entity_model::magma_cube::MODEL,
                &img,
            ));
            info!("slime entity texture bound (magma-cube mesh)");
            break;
        }
    }

    // ---- Magma cube (64x64, entity/slime subdir) ---------------------------
    for root in &entity_roots {
        let p = root.join("slime").join("magmacube.png");
        if let Ok(img) = image::open(&p).map(|i| i.to_rgba8()) {
            let (w, h) = (img.width(), img.height());
            mm.magma_cube_model = Some(crate::entity_model::bake(
                r,
                &crate::entity_model::magma_cube::MODEL,
                &img,
            ));
            info!(
                texture = format!("{w}x{h}"),
                "magma cube entity texture bound (universal model)"
            );
            break;
        }
    }

    // ---- Chicken (64x32) ---------------------------------------------------
    if let Some(img) = try_open_entity(&entity_roots,"chicken", biome) {
        let (w, h) = (img.width(), img.height());
        mm.chicken_head      = Some(upload_part(r, "voxel_demo_mp::chicken_head",      MobPart::ChickenHead,     &img));
        mm.chicken_body      = Some(upload_part(r, "voxel_demo_mp::chicken_body",      MobPart::ChickenBody,     &img));
        mm.chicken_right_leg = Some(upload_part(r, "voxel_demo_mp::chicken_right_leg", MobPart::ChickenRightLeg, &img));
        mm.chicken_left_leg  = Some(upload_part(r, "voxel_demo_mp::chicken_left_leg",  MobPart::ChickenLeftLeg,  &img));
        // W92 - universal MC ChickenModel (beak/wattle/wings). Render prefers this.
        mm.chicken_model = Some(crate::entity_model::bake(r, &crate::entity_model::chicken::MODEL, &img));
        info!(texture = format!("{w}x{h}"), "chicken entity texture bound (universal model + 4 legacy parts)");
    }

    // ---- Wolf (64x32) ------------------------------------------------------
    if let Some(img) = try_open_entity(&entity_roots, "wolf", biome) {
        let (w, h) = (img.width(), img.height());
        // W158 - universal MC WolfModel only (no legacy parts; the
        // render falls back to the generic cube pair when absent).
        mm.wolf_model = Some(crate::entity_model::bake(r, &crate::entity_model::wolf::MODEL, &img));
        info!(texture = format!("{w}x{h}"), "wolf entity texture bound (universal model)");
    }

    // ---- Polar bear (128x64) -----------------------------------------------
    // W161: MC ships it at entity/bear/polarbear.png, NOT a per-mob
    // subdir matching the mob name, so try_open_entity ("polarbear")
    // would miss it. Direct open across the search roots (it's a .png,
    // so the W141 .tga-only alpha flatten doesn't apply).
    for root in &entity_roots {
        let p = root.join("bear").join("polarbear.png");
        if let Ok(img) = image::open(&p).map(|i| i.to_rgba8()) {
            let (w, h) = (img.width(), img.height());
            mm.polar_bear_model =
                Some(crate::entity_model::bake(r, &crate::entity_model::polar_bear::MODEL, &img));
            info!(texture = format!("{w}x{h}"), "polar bear entity texture bound (universal model)");
            break;
        }
    }

    // ---- Rabbit (64x64) ----------------------------------------------------
    // W162: MC ships colour variants (rabbit_brown / _white / _gold /
    // ...); brown is the default. The stem doesn't match the mob name,
    // so open it directly like the polar bear. First NESTED model -
    // exercises the W162 bone-parent chain.
    for root in &entity_roots {
        let p = root.join("rabbit").join("rabbit_brown.png");
        if let Ok(img) = image::open(&p).map(|i| i.to_rgba8()) {
            let (w, h) = (img.width(), img.height());
            mm.rabbit_model =
                Some(crate::entity_model::bake(r, &crate::entity_model::rabbit::MODEL, &img));
            info!(texture = format!("{w}x{h}"), "rabbit entity texture bound (universal nested model)");
            break;
        }
    }

    // ---- Pig (64x64) -------------------------------------------------------
    if let Some(img) = try_open_entity(&entity_roots,"pig", biome) {
        let (w, h) = (img.width(), img.height());
        mm.pig_head           = Some(upload_part(r, "voxel_demo_mp::pig_head",           MobPart::PigHead,          &img));
        mm.pig_body           = Some(upload_part(r, "voxel_demo_mp::pig_body",           MobPart::PigBody,          &img));
        mm.pig_right_front_leg = Some(upload_part(r, "voxel_demo_mp::pig_rf_leg", MobPart::PigRightFrontLeg, &img));
        mm.pig_left_front_leg  = Some(upload_part(r, "voxel_demo_mp::pig_lf_leg", MobPart::PigLeftFrontLeg,  &img));
        mm.pig_right_back_leg  = Some(upload_part(r, "voxel_demo_mp::pig_rb_leg", MobPart::PigRightBackLeg,  &img));
        mm.pig_left_back_leg   = Some(upload_part(r, "voxel_demo_mp::pig_lb_leg", MobPart::PigLeftBackLeg,   &img));
        // W92 - universal MC PigModel (incl. snout). Render prefers this.
        mm.pig_model = Some(crate::entity_model::bake(r, &crate::entity_model::pig::MODEL, &img));
        info!(texture = format!("{w}x{h}"), "pig entity texture bound (universal model + 6 legacy parts)");
    }

    // ---- Cow (64x64) -------------------------------------------------------
    if let Some(img) = try_open_entity(&entity_roots,"cow", biome) {
        let (w, h) = (img.width(), img.height());
        mm.cow_head            = Some(upload_part(r, "voxel_demo_mp::cow_head",    MobPart::CowHead,          &img));
        mm.cow_body            = Some(upload_part(r, "voxel_demo_mp::cow_body",    MobPart::CowBody,          &img));
        mm.cow_right_front_leg = Some(upload_part(r, "voxel_demo_mp::cow_rf_leg", MobPart::CowRightFrontLeg, &img));
        mm.cow_left_front_leg  = Some(upload_part(r, "voxel_demo_mp::cow_lf_leg", MobPart::CowLeftFrontLeg,  &img));
        mm.cow_right_back_leg  = Some(upload_part(r, "voxel_demo_mp::cow_rb_leg", MobPart::CowRightBackLeg,  &img));
        mm.cow_left_back_leg   = Some(upload_part(r, "voxel_demo_mp::cow_lb_leg", MobPart::CowLeftBackLeg,   &img));
        // W91 - bake the universal MC CowModel (horns + body xRot) from the
        // same texture. Render prefers this; the six parts above are the
        // legacy fallback path (removed once all mobs move over in W92).
        mm.cow_model = Some(crate::entity_model::bake(r, &crate::entity_model::cow::MODEL, &img));
        info!(texture = format!("{w}x{h}"), "cow entity texture bound (universal model + 6 legacy parts)");
    }

    // ---- Skeleton (64x32) --------------------------------------------------
    if let Some(img) = try_open_entity(&entity_roots,"skeleton", biome) {
        let (w, h) = (img.width(), img.height());
        mm.skeleton_head      = Some(upload_part(r, "voxel_demo_mp::skeleton_head",      MobPart::SkeletonHead,     &img));
        mm.skeleton_body      = Some(upload_part(r, "voxel_demo_mp::skeleton_body",      MobPart::SkeletonBody,     &img));
        mm.skeleton_right_arm = Some(upload_part(r, "voxel_demo_mp::skeleton_right_arm", MobPart::SkeletonRightArm, &img));
        mm.skeleton_left_arm  = Some(upload_part(r, "voxel_demo_mp::skeleton_left_arm",  MobPart::SkeletonLeftArm,  &img));
        mm.skeleton_right_leg = Some(upload_part(r, "voxel_demo_mp::skeleton_right_leg", MobPart::SkeletonRightLeg, &img));
        mm.skeleton_left_leg  = Some(upload_part(r, "voxel_demo_mp::skeleton_left_leg",  MobPart::SkeletonLeftLeg,  &img));
        // W92 - universal MC SkeletonModel. Render prefers this.
        mm.skeleton_model = Some(crate::entity_model::bake(r, &crate::entity_model::skeleton::MODEL, &img));
        info!(texture = format!("{w}x{h}"), "skeleton entity texture bound (universal model + 6 legacy parts)");
    }

    // ---- Spider (64x32) ----------------------------------------------------
    if let Some(img) = try_open_entity(&entity_roots,"spider", biome) {
        let (w, h) = (img.width(), img.height());
        mm.spider_head  = Some(upload_part(r, "voxel_demo_mp::spider_head",  MobPart::SpiderHead,  &img));
        mm.spider_body0 = Some(upload_part(r, "voxel_demo_mp::spider_body0", MobPart::SpiderBody0, &img));
        mm.spider_body1 = Some(upload_part(r, "voxel_demo_mp::spider_body1", MobPart::SpiderBody1, &img));
        mm.spider_leg   = Some(upload_part(r, "voxel_demo_mp::spider_leg",   MobPart::SpiderLeg,   &img));
        // W92 - universal MC SpiderModel (8 splayed legs). Render prefers this.
        mm.spider_model = Some(crate::entity_model::bake(r, &crate::entity_model::spider::MODEL, &img));
        info!(texture = format!("{w}x{h}"), "spider entity texture bound (universal model + 4 legacy parts)");
    }

    mm
}

/// Periodic chicken feather-lay gate (Q.3). Free fn so the test
/// harness can drive it without a full `Voxel`. Returns `true`
/// exactly on the tick the timer crosses the 12 s mark with the
/// debug toggle on AND a chicken alive on a host; the caller's
/// job is to perform the actual pickup. Resets the accumulator
/// every gated-off frame so flipping the toggle never emits a
/// stale-stored feather.
pub fn tick_chicken_feather_lay(
    debug_enabled: bool,
    host_role: bool,
    chicken_alive: bool,
    accum: &mut f32,
    dt: f32,
) -> bool {
    if !(debug_enabled && host_role && chicken_alive) {
        *accum = 0.0;
        return false;
    }
    *accum += dt;
    if *accum >= 12.0 {
        *accum = 0.0;
        return true;
    }
    false
}
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
