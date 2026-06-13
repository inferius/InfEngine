//! W157 - first-person held-item viewmodel.
//!
//! Renders the player's currently-selected hotbar item in the lower
//! right of the view, MC-style. Two shapes:
//!
//! - **Block items** (`ItemType.places = Some`) render as a small 3D
//!   cube textured from the shared block atlas (top / sides / bottom
//!   tiles composited into one albedo strip). Atlas tiles are already
//!   force-opaque, which is exactly right for a solid block.
//! - **Flat items** (tools, weapons, ingots, food) render as their
//!   item sprite. These load the real PNG off disk so the alpha-cutout
//!   silhouette survives - the atlas force-opaques those tiles, so a
//!   pickaxe cropped from the atlas would be a solid square.
//!
//! The mesh is built lazily per item id and cached. Drawing goes
//! through `Renderer::draw_mesh_viewmodel`, a depth-cleared pass, so
//! the item paints on top of the world and never clips into a wall.
//!
//! The camera-space pose approximates MC's `firstperson_righthand`
//! display transform; the swing/bob animation layers on top in W157.3.

use std::collections::HashMap;
use std::path::Path;

use glam::{Mat4, Vec3};
use infengine_render::{MeshHandle, MeshVertex, Renderer};

use crate::block::{TILE_PX, block_type};
use crate::item::item_type;

/// A cached held-item mesh + whether it draws as a 3D block cube.
#[derive(Clone, Copy)]
pub(crate) struct HeldMesh {
    pub mesh: MeshHandle,
    pub is_block: bool,
}

/// Per-item-id mesh cache. `None` value = the item resolved to no
/// renderable texture (so we don't retry the disk / atlas lookup).
pub(crate) type HeldMeshCache = HashMap<u16, Option<HeldMesh>>;

/// Build (or fetch from cache) the held-item mesh for `item_id`, then
/// queue it as a first-person viewmodel. No-op when the item resolves
/// to no renderable texture (cached as `None` so we try once only).
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_held_item(
    renderer: &mut Renderer,
    cache: &mut HeldMeshCache,
    atlas: Option<&(u32, u32, Vec<u8>)>,
    tex_root: Option<&Path>,
    item_id: u16,
    cam_pos: Vec3,
    cam_target: Vec3,
) {
    let held = match cache.get(&item_id) {
        Some(h) => *h,
        None => {
            let built = build_held_mesh(renderer, atlas, tex_root, item_id);
            cache.insert(item_id, built);
            built
        }
    };
    let Some(held) = held else {
        return;
    };

    // Camera basis - read back from the live camera so the item rides
    // along with pitch + head-bob automatically.
    let fwd = (cam_target - cam_pos).normalize_or_zero();
    if fwd.length_squared() < 1e-6 {
        return;
    }
    let right = fwd.cross(Vec3::Y).normalize_or_zero();
    if right.length_squared() < 1e-6 {
        return; // looking straight up / down - skip this frame.
    }
    let up = right.cross(fwd);
    // Columns: a point (lx, ly, lz, 1) maps to cam_pos + lx*right +
    // ly*up + lz*fwd. lz > 0 sits in front of the camera.
    let cam_basis = Mat4::from_cols(
        right.extend(0.0),
        up.extend(0.0),
        fwd.extend(0.0),
        cam_pos.extend(1.0),
    );

    let model = cam_basis * held_local_transform(held.is_block);
    // Slightly emissive-free, matte: the viewmodel reads its tint as
    // plain albedo. Held items pick up the scene lighting roughly via
    // their camera-anchored world position.
    renderer.draw_mesh_viewmodel(held.mesh, model.to_cols_array(), [1.0; 4], 0.0, 0.85);
}

/// Fixed camera-space pose placing the item in the lower-right of the
/// view. Approximates MC's `firstperson_righthand` display transform
/// (blocks: yaw 45 deg, ~0.4 scale; flat items: a tilted tool grip).
/// Hand-tuned pending the full MC arm-base port + the W157.3 swing.
fn held_local_transform(is_block: bool) -> Mat4 {
    use std::f32::consts::PI;
    if is_block {
        Mat4::from_translation(Vec3::new(0.38, -0.34, 0.52))
            * Mat4::from_rotation_y(PI * 0.25)
            * Mat4::from_rotation_x(PI * 0.06)
            * Mat4::from_scale(Vec3::splat(0.42))
    } else {
        Mat4::from_translation(Vec3::new(0.42, -0.36, 0.52))
            * Mat4::from_rotation_z(PI * 0.13)
            * Mat4::from_rotation_y(-PI * 0.10)
            * Mat4::from_scale(Vec3::splat(0.62))
    }
}

// ----------------------------------------------------------------
// Mesh construction.
// ----------------------------------------------------------------

fn build_held_mesh(
    renderer: &mut Renderer,
    atlas: Option<&(u32, u32, Vec<u8>)>,
    tex_root: Option<&Path>,
    item_id: u16,
) -> Option<HeldMesh> {
    let it = item_type(item_id)?;
    if let Some(block_id) = it.places {
        // Block item: textured cube from the shared atlas tiles.
        let (aw, _ah, rgba) = atlas?;
        let bt = block_type(block_id)?;
        let albedo = composite_block_albedo(*aw, rgba, bt);
        let (verts, indices) = block_cube_geometry();
        let mesh = renderer.create_mesh("held_block", &verts, &indices);
        let a = renderer.make_mesh_albedo(TILE_PX, TILE_PX * 3, &albedo);
        renderer.bind_mesh_albedo_arc(mesh, a);
        Some(HeldMesh { mesh, is_block: true })
    } else {
        // Flat item: load the real sprite PNG so the alpha survives.
        let img = load_item_sprite(tex_root?, item_id)?;
        let (verts, indices) = flat_sprite_geometry();
        let mesh = renderer.create_mesh("held_item", &verts, &indices);
        let a = renderer.make_mesh_albedo(img.width(), img.height(), &img);
        renderer.bind_mesh_albedo_arc(mesh, a);
        Some(HeldMesh { mesh, is_block: false })
    }
}

/// Composite a block's top / sides / bottom tiles into a vertical
/// strip (`TILE_PX` wide, `3*TILE_PX` tall): row 0 = top, row 1 =
/// sides, row 2 = bottom. The cube UVs index the matching third.
fn composite_block_albedo(
    aw: u32,
    rgba: &[u8],
    bt: &crate::block::BlockType,
) -> Vec<u8> {
    let mut out = vec![0u8; (TILE_PX * TILE_PX * 3 * 4) as usize];
    let faces = [&bt.top, &bt.sides, &bt.bottom];
    for (fi, spec) in faces.iter().enumerate() {
        let ox = spec.col * TILE_PX;
        let oy = spec.row * TILE_PX;
        for ty in 0..TILE_PX {
            for tx in 0..TILE_PX {
                let sidx = (((oy + ty) * aw + ox + tx) * 4) as usize;
                let dy = fi as u32 * TILE_PX + ty;
                let didx = ((dy * TILE_PX + tx) * 4) as usize;
                if sidx + 4 <= rgba.len() {
                    out[didx..didx + 4].copy_from_slice(&rgba[sidx..sidx + 4]);
                }
            }
        }
    }
    out
}

/// Resolve + load the item sprite PNG. The filename comes from the
/// block-atlas registry row (`top.files[0]`, e.g. `diamond_pickaxe.png`)
/// with the item name as a fallback; searched under `textures/item/`
/// then `textures/block/`.
fn load_item_sprite(tex_root: &Path, item_id: u16) -> Option<image::RgbaImage> {
    let mut candidates: Vec<String> = Vec::new();
    if let Some(bt) = block_type(item_id)
        && let Some(f) = bt.top.files.first()
    {
        candidates.push((*f).to_string());
    }
    if let Some(it) = item_type(item_id) {
        candidates.push(format!("{}.png", it.name));
    }
    for file in candidates {
        for sub in ["item", "block"] {
            let p = tex_root.join(sub).join(&file);
            if let Ok(img) = image::open(&p) {
                return Some(img.to_rgba8());
            }
        }
    }
    None
}

/// A double-sided unit quad in the local XY plane (z = 0), spanning
/// [-0.5, 0.5]^2. UVs map the bound sprite with v = 0 at the top.
/// Double-wound so it shows from either side under back-face culling.
fn flat_sprite_geometry() -> (Vec<MeshVertex>, Vec<u32>) {
    let n = [0.0, 0.0, 1.0];
    // (BL, BR, TR, TL) with v: bottom = 1, top = 0.
    let corners = [
        [-0.5, -0.5, 0.0],
        [0.5, -0.5, 0.0],
        [0.5, 0.5, 0.0],
        [-0.5, 0.5, 0.0],
    ];
    let uvs = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    let mut verts = Vec::with_capacity(4);
    for i in 0..4 {
        verts.push(MeshVertex::new(corners[i], n).with_uv(uvs[i]).with_light(0.0, 1.0));
    }
    // Front winding + reversed winding = visible from both sides.
    let indices = vec![0, 1, 2, 0, 2, 3, 0, 2, 1, 0, 3, 2];
    (verts, indices)
}

/// A unit cube centered at the origin ([-0.5, 0.5]^3), single CCW-
/// outward winding (the opaque viewmodel pipeline culls back faces).
/// Faces UV into the composited albedo strip: top -> v[0, 1/3], the
/// four sides -> v[1/3, 2/3], bottom -> v[2/3, 1].
fn block_cube_geometry() -> (Vec<MeshVertex>, Vec<u32>) {
    let mut verts: Vec<MeshVertex> = Vec::with_capacity(24);
    let mut indices: Vec<u32> = Vec::with_capacity(36);
    let third = 1.0 / 3.0;
    // (normal, u_axis, v_axis, v_lo, v_hi). cross(u_axis, v_axis) = normal.
    type FaceDef = ([f32; 3], [f32; 3], [f32; 3], f32, f32);
    let faces: [FaceDef; 6] = [
        ([0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, -1.0], 0.0, third), // top
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 2.0 * third, 1.0), // bottom
        ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0], third, 2.0 * third), // +X
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], third, 2.0 * third), // -X
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], third, 2.0 * third), // +Z
        ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], third, 2.0 * third), // -Z
    ];
    let h = 0.5;
    for (normal, u_axis, v_axis, v_lo, v_hi) in faces {
        let u = Vec3::from(u_axis) * h;
        let v = Vec3::from(v_axis) * h;
        // Corners CCW from outside: -u-v, +u-v, +u+v, -u+v.
        let c = [-u - v, u - v, u + v, -u + v];
        // v_hi at the bottom (-v_axis), v_lo at the top (+v_axis).
        let uvs = [[0.0, v_hi], [1.0, v_hi], [1.0, v_lo], [0.0, v_lo]];
        let base = verts.len() as u32;
        for i in 0..4 {
            verts.push(
                MeshVertex::new(c[i].to_array(), normal)
                    .with_uv(uvs[i])
                    .with_light(0.0, 1.0),
            );
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (verts, indices)
}
