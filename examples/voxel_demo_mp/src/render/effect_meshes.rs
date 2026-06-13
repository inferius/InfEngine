//! W153 - textured fire + nether portal meshes.
//!
//! Both blocks used to render as PROCEDURALLY TINTED cube slabs
//! (solid purple / solid orange - the user's "divoka textura"
//! report). They now bake real MC textures: `nether_portal.png` and
//! `fire_0.png` are vertical animation strips, so the bake crops
//! FRAME 0 into a dedicated albedo. In-place strip animation is the
//! follow-up (needs the atlas-bound anim path on the mesh pipeline);
//! a static MC frame already reads correctly at gameplay distance.

use infengine_render::{MeshHandle, MeshVertex, Renderer};

/// Baked fire + portal meshes. `None` fields fall back to the
/// legacy tinted-slab draw.
#[derive(Default)]
pub(crate) struct EffectMeshes {
    pub(crate) portal: Option<MeshHandle>,
    pub(crate) fire: Option<MeshHandle>,
}

/// Crop frame 0 (the top `w` x `w` tile) of a vertical strip.
fn frame0(img: &image::RgbaImage) -> image::RgbaImage {
    let w = img.width();
    let h = img.height().min(w.max(1));
    image::imageops::crop_imm(img, 0, 0, w, h).to_image()
}

/// Push one double-wound quad into the buffers.
fn push_quad(
    verts: &mut Vec<MeshVertex>,
    idxs: &mut Vec<u32>,
    corners: [[f32; 3]; 4],
    normal: [f32; 3],
) {
    let base = verts.len() as u32;
    let uvs = [[0.0f32, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    for i in 0..4 {
        verts.push(
            MeshVertex::new(corners[i], normal)
                .with_uv(uvs[i])
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
fn single_panel(r: &mut Renderer, label: &str) -> MeshHandle {
    let mut verts: Vec<MeshVertex> = Vec::new();
    let mut idxs: Vec<u32> = Vec::new();
    push_quad(
        &mut verts,
        &mut idxs,
        [[-0.5, 0.0, 0.0], [0.5, 0.0, 0.0], [0.5, 1.0, 0.0], [-0.5, 1.0, 0.0]],
        [0.0, 0.0, 1.0],
    );
    r.create_mesh(label, &verts, &idxs)
}

/// W159: MC's fire_floor model - four planes that LEAN inward (the
/// top edge of each sheet is offset toward the cell centre), so the
/// flame reads as a tepee of tilted sheets, not a rigid vertical X.
/// Mirrors `models/block/template_fire_floor.json`. Double-wound.
fn fire_quads(r: &mut Renderer, label: &str) -> MeshHandle {
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
        push_quad(&mut verts, &mut idxs, *s, *n);
    }
    r.create_mesh(label, &verts, &idxs)
}

/// Load + bake the two effect meshes from the pack's block texture
/// dir. Missing files leave the legacy tinted fallback in place.
pub(crate) fn load_effect_meshes(
    r: &mut Renderer,
    block_tex_dir: Option<&std::path::Path>,
) -> EffectMeshes {
    let mut out = EffectMeshes::default();
    let Some(dir) = block_tex_dir else { return out };
    #[derive(Clone, Copy)]
    enum Shape {
        Portal,
        Fire,
    }
    let mut bake = |stem: &str, label: &str, shape: Shape| -> Option<MeshHandle> {
        let p = dir.join(format!("{stem}.png"));
        match image::open(&p) {
            Ok(img) => {
                let f0 = frame0(&img.to_rgba8());
                let albedo = r.make_mesh_albedo(f0.width(), f0.height(), &f0);
                let mesh = match shape {
                    Shape::Portal => single_panel(r, label),
                    Shape::Fire => fire_quads(r, label),
                };
                r.bind_mesh_albedo_arc(mesh, albedo);
                Some(mesh)
            }
            Err(err) => {
                tracing::warn!(error = %err, path = %p.display(), "effect sheet load failed");
                None
            }
        }
    };
    // W156: the portal is ONE flat sheet (rotated per frame axis by
    // the draw). W159: fire is MC's leaning fire_floor tepee, not a
    // rigid vertical cross.
    out.portal = bake("nether_portal", "voxel_demo_mp::portal_panel", Shape::Portal);
    out.fire = bake("fire_0", "voxel_demo_mp::fire_cross", Shape::Fire);
    tracing::info!(
        portal = out.portal.is_some(),
        fire = out.fire.is_some(),
        "fire + portal effect meshes baked",
    );
    out
}
