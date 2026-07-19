//! W153 / W320 - textured fire + nether portal meshes.
//!
//! Both blocks used to render as PROCEDURALLY TINTED cube slabs
//! (solid purple / solid orange - the user's "divoka textura"
//! report). W153 baked frame 0 of the real MC strips into dedicated
//! albedos. W320 finishes the job: the meshes now carry ATLAS UVs +
//! the tile-anim slot (exactly like the W200 liquid meshes), so the
//! GPU walks the full `fire_0` / `soul_fire_0` / `nether_portal`
//! mcmeta strip per frame - live flame + swirl animation, no
//! per-frame re-upload. Soul fire gets its own mesh from the real
//! `soul_fire_0` art instead of a cyan-tinted orange flame.

use infengine_render::{AlbedoHandle, MeshHandle, MeshVertex, Renderer};

use crate::resource_pack::{ResourcePack, TextureKey};

/// Baked fire + portal meshes. `None` fields fall back to the
/// legacy tinted-slab draw.
#[derive(Default)]
pub(crate) struct EffectMeshes {
    pub(crate) portal: Option<MeshHandle>,
    pub(crate) fire: Option<MeshHandle>,
    /// W320: dedicated soul-fire mesh (real `soul_fire_0` texture).
    /// Falls back to `fire` + a cyan tint when absent.
    pub(crate) soul_fire: Option<MeshHandle>,
}

/// Push one double-wound quad into the buffers. `tile` is the
/// frame-0 atlas rect `[u0, v0, u1, v1]`; `anim` the tile-anim slot
/// (0 = static). Corner order is bottom-left, bottom-right,
/// top-right, top-left; the V axis flips so texture row 0 (top of
/// the MC art) lands at the TOP of the quad.
fn push_quad(
    verts: &mut Vec<MeshVertex>,
    idxs: &mut Vec<u32>,
    corners: [[f32; 3]; 4],
    normal: [f32; 3],
    tile: [f32; 4],
    anim: u32,
) {
    let base = verts.len() as u32;
    let uv0 = [tile[0], tile[1]];
    let size = [tile[2] - tile[0], tile[3] - tile[1]];
    let corner_uv = [[0.0f32, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    for i in 0..4 {
        verts.push(
            MeshVertex::new(corners[i], normal)
                .with_tile_uv(uv0, size, corner_uv[i])
                .with_anim_id(anim)
                .with_light(0.0, 1.0),
        );
    }
    idxs.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    idxs.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
}

/// W156: a SINGLE full-cell panel on the Z plane (faces +-Z),
/// double-wound. The portal draw rotates it 90 deg for frames whose
/// curtain runs along Z - MC portals are one flat sheet in the
/// frame plane, never a cross.
fn single_panel(r: &mut Renderer, label: &str, tile: [f32; 4], anim: u32) -> MeshHandle {
    let mut verts: Vec<MeshVertex> = Vec::new();
    let mut idxs: Vec<u32> = Vec::new();
    push_quad(
        &mut verts,
        &mut idxs,
        [[-0.5, 0.0, 0.0], [0.5, 0.0, 0.0], [0.5, 1.0, 0.0], [-0.5, 1.0, 0.0]],
        [0.0, 0.0, 1.0],
        tile,
        anim,
    );
    r.create_mesh(label, &verts, &idxs)
}

/// W159: MC's fire_floor model - four planes that LEAN inward (the
/// top edge of each sheet is offset toward the cell centre), so the
/// flame reads as a tepee of tilted sheets, not a rigid vertical X.
/// Mirrors `models/block/template_fire_floor.json`. Double-wound.
fn fire_quads(r: &mut Renderer, label: &str, tile: [f32; 4], anim: u32) -> MeshHandle {
    let mut verts: Vec<MeshVertex> = Vec::new();
    let mut idxs: Vec<u32> = Vec::new();
    let h = 1.4_f32; // MC fire pokes ~1.4 cells tall
    let lean = 0.5_f32; // top edge offset toward centre
    // Two sheets leaning along Z (bottom at -Z / +Z edges, top
    // pulled to centre) + two leaning along X. Each spans the cell.
    let sheets: [[[f32; 3]; 4]; 4] = [
        // bottom edge at z=-0.5, top edge leaned to z=-0.5+lean.
        [[-0.5, 0.0, -0.5], [0.5, 0.0, -0.5], [0.5, h, -0.5 + lean], [-0.5, h, -0.5 + lean]],
        // bottom edge at z=+0.5, top leaned to +0.5-lean.
        [[-0.5, 0.0, 0.5], [0.5, 0.0, 0.5], [0.5, h, 0.5 - lean], [-0.5, h, 0.5 - lean]],
        // bottom edge at x=-0.5, top leaned to -0.5+lean.
        [[-0.5, 0.0, -0.5], [-0.5, 0.0, 0.5], [-0.5 + lean, h, 0.5], [-0.5 + lean, h, -0.5]],
        // bottom edge at x=+0.5, top leaned to +0.5-lean.
        [[0.5, 0.0, -0.5], [0.5, 0.0, 0.5], [0.5 - lean, h, 0.5], [0.5 - lean, h, -0.5]],
    ];
    let normals = [[0.0, 0.0, 1.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0], [-1.0, 0.0, 0.0]];
    for (s, n) in sheets.iter().zip(normals.iter()) {
        push_quad(&mut verts, &mut idxs, *s, *n, tile, anim);
    }
    r.create_mesh(label, &verts, &idxs)
}

/// Build the effect meshes against the loaded pack atlas. Each mesh
/// samples its texture's frame-0 rect + carries the tile-anim slot,
/// so the shared `tile_anims` uniform animates it like any chunk
/// tile. A missing texture (or the fallback pack / no atlas handle)
/// leaves the field `None` -> the legacy tinted draw stays alive.
pub(crate) fn load_effect_meshes(
    r: &mut Renderer,
    pack: &ResourcePack,
    atlas: Option<&AlbedoHandle>,
) -> EffectMeshes {
    let mut out = EffectMeshes::default();
    if pack.is_fallback {
        return out;
    }
    let Some(atlas) = atlas else { return out };
    #[derive(Clone, Copy)]
    enum Shape {
        Portal,
        Fire,
    }
    let mut bake = |stem: &str, label: &str, shape: Shape| -> Option<MeshHandle> {
        let key = TextureKey::Block(stem.to_string());
        let Some(tex) = pack.textures.get(&key) else {
            tracing::warn!(stem, "effect texture missing from atlas");
            return None;
        };
        let anim = pack.anim_slot(&key) as u32;
        let mesh = match shape {
            Shape::Portal => single_panel(r, label, tex.atlas_uv, anim),
            Shape::Fire => fire_quads(r, label, tex.atlas_uv, anim),
        };
        r.bind_mesh_albedo_arc(mesh, atlas.clone());
        Some(mesh)
    };
    // W156: the portal is ONE flat sheet (rotated per frame axis by
    // the draw). W159: fire is MC's leaning fire_floor tepee, not a
    // rigid vertical cross. W320: all three animate via anim slots.
    out.portal = bake("nether_portal", "voxel_demo_mp::portal_panel", Shape::Portal);
    out.fire = bake("fire_0", "voxel_demo_mp::fire_cross", Shape::Fire);
    out.soul_fire = bake("soul_fire_0", "voxel_demo_mp::soul_fire_cross", Shape::Fire);
    tracing::info!(
        portal = out.portal.is_some(),
        fire = out.fire.is_some(),
        soul_fire = out.soul_fire.is_some(),
        "fire + portal effect meshes baked",
    );
    out
}
