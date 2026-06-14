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

/// First-person swing + equip animation state. Ticked once per frame
/// from the input state; the two progress values drive the held-item
/// transform.
#[derive(Clone, Copy, Default)]
pub(crate) struct HeldAnim {
    /// 0 = idle, otherwise 0..1 sweeping through one swing arc.
    pub swing_progress: f32,
    /// 1 = item just equipped (raising from below), decays to 0.
    pub equip_progress: f32,
    /// Last held item id - a change retriggers the equip raise.
    pub last_held_id: u16,
    /// Previous RMB-held state, for rising-edge "use" detection.
    pub prev_rmb: bool,
}

/// One swing lasts ~6 MC ticks (0.3 s); equip raise is quicker.
const SWING_DUR: f32 = 0.3;
const EQUIP_DUR: f32 = 0.15;

/// Advance the swing + equip clocks and (re)trigger a swing. Call once
/// per frame regardless of camera mode so the state stays coherent.
/// `lmb_held` swings continuously (mining / attacking); a rising RMB
/// edge swings once (use / place). New swings are suppressed during
/// the equip raise so a fresh item settles before it can swing.
pub(crate) fn tick_held_anim(
    a: &mut HeldAnim,
    dt: f32,
    lmb_held: bool,
    rmb_held: bool,
    held_id: u16,
) {
    if held_id != a.last_held_id {
        a.equip_progress = 1.0;
        a.last_held_id = held_id;
        a.swing_progress = 0.0;
    }
    if a.equip_progress > 0.0 {
        a.equip_progress = (a.equip_progress - dt / EQUIP_DUR).max(0.0);
    }
    if a.swing_progress > 0.0 {
        a.swing_progress += dt / SWING_DUR;
        if a.swing_progress >= 1.0 {
            a.swing_progress = 0.0;
        }
    }
    let rmb_edge = rmb_held && !a.prev_rmb;
    a.prev_rmb = rmb_held;
    if a.swing_progress == 0.0 && a.equip_progress < 0.5 && (lmb_held || rmb_edge) {
        a.swing_progress = 1e-4;
    }
}

/// Build (or fetch from cache) the held-item mesh for `item_id`, then
/// queue it as a first-person viewmodel. No-op when the item resolves
/// to no renderable texture (cached as `None` so we try once only).
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_held_item(
    renderer: &mut Renderer,
    cache: &mut HeldMeshCache,
    tex_root: Option<&Path>,
    item_id: u16,
    cam_pos: Vec3,
    cam_target: Vec3,
    swing: f32,
    equip: f32,
) {
    let held = match cache.get(&item_id) {
        Some(h) => *h,
        None => {
            let built = build_held_mesh(renderer, tex_root, item_id);
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

    let model = cam_basis * held_local_transform(held.is_block, swing, equip);
    // Matte: the viewmodel reads its tint as plain albedo. Held items
    // pick up the scene lighting roughly via their camera world pos.
    renderer.draw_mesh_viewmodel(held.mesh, model.to_cols_array(), [1.0; 4], 0.0, 0.85);
}

/// Camera-space pose placing the item in the lower-right of the view,
/// with the swing + equip animation layered on. Approximates MC's
/// `firstperson_righthand` display transform (blocks: yaw 45 deg,
/// ~0.4 scale; flat items: a tilted tool grip), hand-tuned to sit on
/// screen with the shared camera (MC's raw arm offsets assume a wider
/// hand projection). The swing uses MC's easing curves
/// (`sin(swing^2 * PI)` jab + `sin(sqrt(swing) * PI)` arc).
fn held_local_transform(is_block: bool, swing: f32, equip: f32) -> Mat4 {
    use std::f32::consts::PI;
    let (base, display) = if is_block {
        (
            Vec3::new(0.38, -0.34, 0.52),
            Mat4::from_rotation_y(PI * 0.25)
                * Mat4::from_rotation_x(PI * 0.06)
                * Mat4::from_scale(Vec3::splat(0.42)),
        )
    } else {
        (
            Vec3::new(0.42, -0.36, 0.52),
            Mat4::from_rotation_z(PI * 0.13)
                * Mat4::from_rotation_y(-PI * 0.10)
                * Mat4::from_scale(Vec3::splat(0.62)),
        )
    };
    // Equip raise: a freshly-selected item rises from below + back.
    let arm = Mat4::from_translation(base + Vec3::new(0.0, -equip * 0.5, equip * 0.12));
    // Swing arc (identity at swing = 0): the item dips down, jabs
    // forward and pitches over, then eases back - MC's first-person
    // swing in spirit (exact axis-for-axis port pending visual tuning).
    let f = (swing * swing * PI).sin();
    let f1 = (swing.max(0.0).sqrt() * PI).sin();
    let swing_xform = Mat4::from_translation(Vec3::new(0.0, -f1 * 0.18, f * 0.12))
        * Mat4::from_rotation_x(-f1 * 1.3)
        * Mat4::from_rotation_z(f1 * 0.35);
    arm * swing_xform * display
}

// ----------------------------------------------------------------
// Mesh construction.
// ----------------------------------------------------------------

fn build_held_mesh(
    renderer: &mut Renderer,
    tex_root: Option<&Path>,
    item_id: u16,
) -> Option<HeldMesh> {
    let it = item_type(item_id)?;
    let root = tex_root?;
    if let Some(block_id) = it.places {
        // Block item: a small textured cube. W176: composite the
        // top/side/bottom face PNGs straight off disk - the earlier
        // code cropped vx.atlas_rgba at (col,row)*TILE_PX, but that
        // atlas is SHELF-PACKED by name (not a col/row grid), so the
        // crop read garbage pixels = the "weird thing" the user saw.
        let bt = block_type(block_id)?;
        let albedo = composite_block_albedo_from_disk(root, bt)?;
        let (verts, indices) = block_cube_geometry();
        let mesh = renderer.create_mesh("held_block", &verts, &indices);
        let a = renderer.make_mesh_albedo(TILE_PX, TILE_PX * 3, &albedo);
        renderer.bind_mesh_albedo_arc(mesh, a);
        Some(HeldMesh { mesh, is_block: true })
    } else {
        // Flat item: load the real sprite PNG so the alpha survives.
        let img = load_item_sprite(root, item_id)?;
        let (verts, indices) = flat_sprite_geometry();
        let mesh = renderer.create_mesh("held_item", &verts, &indices);
        let a = renderer.make_mesh_albedo(img.width(), img.height(), &img);
        renderer.bind_mesh_albedo_arc(mesh, a);
        Some(HeldMesh { mesh, is_block: false })
    }
}

/// W176: composite a block's top / sides / bottom face PNGs (loaded
/// from `<root>/block/<file>`) into a vertical strip (`TILE_PX` wide,
/// `3*TILE_PX` tall): row 0 = top, row 1 = sides, row 2 = bottom. The
/// cube UVs index the matching third. Each face is nearest-resized to
/// `TILE_PX`. Returns `None` if any face PNG is missing.
fn composite_block_albedo_from_disk(
    root: &Path,
    bt: &crate::block::BlockType,
) -> Option<Vec<u8>> {
    let load = |file: &str| -> Option<image::RgbaImage> {
        let img = image::open(root.join("block").join(file)).ok()?.to_rgba8();
        Some(if img.width() == TILE_PX && img.height() == TILE_PX {
            img
        } else {
            image::imageops::resize(
                &img,
                TILE_PX,
                TILE_PX,
                image::imageops::FilterType::Nearest,
            )
        })
    };
    let faces = [
        load(bt.top.files.first()?)?,
        load(bt.sides.files.first()?)?,
        load(bt.bottom.files.first()?)?,
    ];
    let mut out = vec![0u8; (TILE_PX * TILE_PX * 3 * 4) as usize];
    for (fi, img) in faces.iter().enumerate() {
        for ty in 0..TILE_PX {
            for tx in 0..TILE_PX {
                let src = img.get_pixel(tx, ty).0;
                let dy = fi as u32 * TILE_PX + ty;
                let didx = ((dy * TILE_PX + tx) * 4) as usize;
                out[didx..didx + 4].copy_from_slice(&src);
            }
        }
    }
    Some(out)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Switching items raises the equip animation to 1 and clears any
    /// in-flight swing, then it decays to 0 over EQUIP_DUR.
    #[test]
    fn equip_raises_on_item_change_then_decays() {
        let mut a = HeldAnim { swing_progress: 0.5, ..Default::default() };
        tick_held_anim(&mut a, 0.0, false, false, 42);
        assert_eq!(a.equip_progress, 1.0, "switch to id 42 raises equip");
        assert_eq!(a.swing_progress, 0.0, "switch cancels the swing");
        assert_eq!(a.last_held_id, 42);
        // Decay to zero.
        for _ in 0..100 {
            tick_held_anim(&mut a, 0.05, false, false, 42);
        }
        assert_eq!(a.equip_progress, 0.0);
    }

    /// Holding LMB starts a swing that advances to completion (back to
    /// idle 0) over roughly SWING_DUR, then immediately restarts while
    /// LMB stays held (continuous mining / attacking).
    #[test]
    fn lmb_held_swings_continuously() {
        // last_held_id = 1 so the first tick sees no item change (no
        // equip lockout).
        let mut a = HeldAnim { last_held_id: 1, ..Default::default() };
        // First tick with LMB down kicks off a swing.
        tick_held_anim(&mut a, 0.0, true, false, 1);
        assert!(a.swing_progress > 0.0, "LMB held starts a swing");
        // Advance most of the way without finishing.
        tick_held_anim(&mut a, SWING_DUR * 0.5, true, false, 1);
        assert!(a.swing_progress > 0.0 && a.swing_progress < 1.0);
        // Overshoot the end: it wraps to 0 then restarts (LMB still held).
        tick_held_anim(&mut a, SWING_DUR, true, false, 1);
        assert!(a.swing_progress > 0.0, "swing restarts while LMB held");
    }

    /// A held RMB swings exactly once - no retrigger until it is
    /// released and pressed again.
    #[test]
    fn rmb_swings_once_per_press() {
        let mut a = HeldAnim { last_held_id: 1, ..Default::default() };
        // Rising edge -> swing.
        tick_held_anim(&mut a, 0.0, false, true, 1);
        assert!(a.swing_progress > 0.0, "RMB press starts a swing");
        // Finish the swing while RMB stays held: must NOT retrigger.
        tick_held_anim(&mut a, SWING_DUR + 0.01, false, true, 1);
        assert_eq!(a.swing_progress, 0.0, "held RMB does not retrigger");
        // Release + press again -> new swing.
        tick_held_anim(&mut a, 0.0, false, false, 1);
        tick_held_anim(&mut a, 0.0, false, true, 1);
        assert!(a.swing_progress > 0.0, "re-press swings again");
    }

    /// No new swing may start while the equip raise is still prominent.
    #[test]
    fn no_swing_during_equip_raise() {
        let mut a = HeldAnim::default();
        // Equip just triggered (full raise); LMB down should be ignored.
        tick_held_anim(&mut a, 0.0, true, false, 7);
        assert_eq!(a.equip_progress, 1.0);
        assert_eq!(a.swing_progress, 0.0, "swing suppressed mid-equip");
    }
}
