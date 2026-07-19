//! W126 - chest block-entity renderer (MC `ChestRenderer`).
//!
//! Chests left the static chunk mesh: the cell emits no geometry
//! (workers + sync emit_special return early) and this pass draws
//! every loaded chest per frame through the shared `entity_model`
//! pipeline with the real `entity/chest/*.png` sheets. That buys:
//! - the correct MC chest texture (not the old oak-planks box),
//! - per-placement facing (latch toward the placer),
//! - the double chest: two adjacent same-facing chests render the
//!   15-wide left/right halves flush across the seam,
//! - the lid open animation (MC ease `1-(1-t)^3`, max PI/2),
//!   driven by the local open UI + host-side viewer registry.

use glam::Vec3;
use infengine_render::{MeshVertex, Renderer};
use std::collections::HashMap;

use crate::entity_model::{self, BakedBone, BakedModel, BonePose, EntityModel, McCube};

/// Baked chest models + their sheets. `Default` = nothing loaded
/// (fallback pack) - the draw pass just skips.
#[derive(Default)]
pub(crate) struct ChestMeshes {
    pub(crate) single: Option<BakedModel>,
    /// Half extending local +X, baked with `normal_left.png`.
    pub(crate) ext_pos: Option<BakedModel>,
    /// Half extending local -X, baked with `normal_right.png`.
    pub(crate) ext_neg: Option<BakedModel>,
}

impl ChestMeshes {
    pub(crate) fn loaded(&self) -> bool {
        self.single.is_some()
    }
}

/// Load + bake the chest models from `textures/entity/chest/`.
pub(crate) fn load_chest_meshes(
    r: &mut Renderer,
    entity_dir: Option<&std::path::Path>,
) -> ChestMeshes {
    let mut out = ChestMeshes::default();
    let Some(dir) = entity_dir else { return out };
    let chest_dir = dir.join("chest");
    let load = |file: &str| -> Option<image::RgbaImage> {
        let p = chest_dir.join(file);
        match image::open(&p) {
            Ok(img) => Some(img.to_rgba8()),
            Err(err) => {
                tracing::warn!(error = %err, path = %p.display(), "chest sheet load failed");
                None
            }
        }
    };
    if let Some(img) = load("normal.png") {
        out.single = Some(bake_chest(r, &entity_model::chest::MODEL, &img));
    }
    // Sheet pairing derives from the transparent SEAM tile: the MC
    // box unwrap maps the "right" tile to the model +X face;
    // normal_right.png leaves its "left" (-X) tile transparent, so
    // it belongs to the half whose seam sits at model -X = EXT_POS
    // (the flip negates X: world +X extension = model -X seam).
    if let Some(img) = load("normal_right.png") {
        out.ext_pos = Some(bake_chest(r, &entity_model::chest::MODEL_EXT_POS, &img));
    }
    if let Some(img) = load("normal_left.png") {
        out.ext_neg = Some(bake_chest(r, &entity_model::chest::MODEL_EXT_NEG, &img));
    }
    tracing::info!(loaded = out.loaded(), "chest entity meshes baked");
    out
}


/// W131: chest-specific bake. The generic mob `append_cube` maps
/// the unwrap "top" slot onto the world-top face with its U axis
/// along Z - fine for mobs, but the chest sheets are authored the
/// other way: the LIGHT plank tile in the "bottom" slot is the
/// closed lid's OUTSIDE, the dark-bordered "top"-slot tile is the
/// lid's INNER face, and the black-bordered base "bottom" tile is
/// the chest's interior floor (verified per-tile against
/// normal.png + the user's report). The horizontal faces here use
/// swapped slots and axis-true UVs (U along X, V along Z), which
/// also un-rotates the double-half tops (15px tile onto the 15px
/// axis). Side faces keep the proven seam-aware mapping.
fn bake_chest(
    r: &mut Renderer,
    model: &EntityModel,
    img: &image::RgbaImage,
) -> BakedModel {
    let albedo = r.make_mesh_albedo(img.width(), img.height(), img);
    let (tw, th) = (img.width() as f32, img.height() as f32);
    let mut bones = Vec::with_capacity(model.bones.len());
    for bone in model.bones {
        let mut verts: Vec<MeshVertex> = Vec::new();
        let mut idxs: Vec<u32> = Vec::new();
        for cube in bone.cubes {
            append_chest_cube(&mut verts, &mut idxs, cube, tw, th);
        }
        let mesh = r.create_mesh(bone.name, &verts, &idxs);
        r.bind_mesh_albedo_arc(mesh, albedo.clone());
        bones.push(BakedBone { bone: *bone, mesh });
    }
    // The chest base sits on the ground by construction (base box
    // spans MC y -10..0 -> world 0..10/16).
    BakedModel { bones, ground_offset: 0.0, model_scale: 1.0 }
}

/// One chest cube with the chest-sheet UV conventions (see
/// `bake_chest`). The tiny latch keeps the fully standard unwrap.
fn append_chest_cube(
    verts: &mut Vec<MeshVertex>,
    idxs: &mut Vec<u32>,
    cube: &McCube,
    tw: f32,
    th: f32,
) {
    let [ox, oy, oz] = cube.origin;
    let [sx, sy, sz] = cube.size;
    let (x0, y0, z0) = (ox, oy, oz);
    let (x1, y1, z1) = (ox + sx, oy + sy, oz + sz);
    let (w, h, d) = (sx, sy, sz);
    let (u0, v0) = (cube.uv[0] as f32, cube.uv[1] as f32);
    let uv = |pu: f32, pv: f32| -> [f32; 2] { [pu / tw, pv / th] };

    let mut add = |corners: [[f32; 3]; 4], normal: [f32; 3], uvs: [[f32; 2]; 4]| {
        let base = verts.len() as u32;
        for i in 0..4 {
            verts.push(
                MeshVertex::new(corners[i], normal)
                    .with_uv(uvs[i])
                    .with_light(0.0, 1.0),
            );
        }
        idxs.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    };

    // Standard unwrap strips (identical to the mob mapping).
    let (r_u0, r_u1) = (u0, u0 + d);
    let (f_u0, f_u1) = (u0 + d, u0 + d + w);
    let (l_u0, l_u1) = (u0 + d + w, u0 + d + w + d);
    let (b_u0, b_u1) = (u0 + d + w + d, u0 + d + w + d + w);
    let sv0 = v0 + d;
    let sv1 = v0 + d + h;
    let (t_u0, t_u1) = (u0 + d, u0 + d + w);
    let (bt_u0, bt_u1) = (u0 + d + w, u0 + d + w + w);
    let (tv0, tv1) = (v0, v0 + d);

    // Front (MC -Z), back, right (+X), left (-X): proven mapping.
    add(
        [[x1, y1, z0], [x0, y1, z0], [x0, y0, z0], [x1, y0, z0]],
        [0.0, 0.0, -1.0],
        [uv(f_u0, sv1), uv(f_u1, sv1), uv(f_u1, sv0), uv(f_u0, sv0)],
    );
    add(
        [[x0, y1, z1], [x1, y1, z1], [x1, y0, z1], [x0, y0, z1]],
        [0.0, 0.0, 1.0],
        [uv(b_u0, sv1), uv(b_u1, sv1), uv(b_u1, sv0), uv(b_u0, sv0)],
    );
    add(
        [[x1, y1, z1], [x1, y1, z0], [x1, y0, z0], [x1, y0, z1]],
        [1.0, 0.0, 0.0],
        [uv(r_u0, sv1), uv(r_u1, sv1), uv(r_u1, sv0), uv(r_u0, sv0)],
    );
    add(
        [[x0, y1, z0], [x0, y1, z1], [x0, y0, z1], [x0, y0, z0]],
        [-1.0, 0.0, 0.0],
        [uv(l_u0, sv1), uv(l_u1, sv1), uv(l_u1, sv0), uv(l_u0, sv0)],
    );

    // The latch is tiny: standard top/bottom, done.
    let is_latch = h == 4.0;
    if is_latch {
        add(
            [[x1, y0, z1], [x1, y0, z0], [x0, y0, z0], [x0, y0, z1]],
            [0.0, -1.0, 0.0],
            [uv(t_u0, tv1), uv(t_u1, tv1), uv(t_u1, tv0), uv(t_u0, tv0)],
        );
        add(
            [[x0, y1, z1], [x0, y1, z0], [x1, y1, z0], [x1, y1, z1]],
            [0.0, 1.0, 0.0],
            [uv(bt_u0, tv1), uv(bt_u1, tv1), uv(bt_u1, tv0), uv(bt_u0, tv0)],
        );
        return;
    }

    // World-TOP face (MC y0): the "bottom"-slot tile (light planks
    // on the lid, plain underside art on the base), axis-true.
    // W132: rotated 180 from the first cut (user report).
    add(
        [[x1, y0, z1], [x1, y0, z0], [x0, y0, z0], [x0, y0, z1]],
        [0.0, -1.0, 0.0],
        [
            uv(bt_u0, tv0),
            uv(bt_u0, tv1),
            uv(bt_u1, tv1),
            uv(bt_u1, tv0),
        ],
    );
    // World-BOTTOM face (MC y1): the "top"-slot tile (the dark
    // bordered lid inner / base interior floor), axis-true.
    // W132: rotated 180 to match.
    add(
        [[x0, y1, z1], [x0, y1, z0], [x1, y1, z0], [x1, y1, z1]],
        [0.0, 1.0, 0.0],
        [
            uv(t_u1, tv0),
            uv(t_u1, tv1),
            uv(t_u0, tv1),
            uv(t_u0, tv0),
        ],
    );
}

/// Chest facing -> entity yaw. Facing codes match `block::door`
/// (0=+X, 1=+Z, 2=-X, 3=-Z = the direction the latch points).
/// The model front is local -Z; yaw 0 keeps it at world -Z.
fn yaw_for_facing(facing: u8) -> f32 {
    use std::f32::consts::{FRAC_PI_2, PI};
    match facing % 4 {
        0 => -FRAC_PI_2, // +X east
        1 => PI,         // +Z south
        2 => FRAC_PI_2,  // -X west
        _ => 0.0,        // -Z north
    }
}

// W128: the EXT_POS extension direction is shared with the UI
// pairing rule - see `block::chest::ext_pos_dir`.
use crate::block::chest::ext_pos_dir;

/// MC lid ease: `1 - (1-t)^3`, angle up to PI/2.
fn lid_angle(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    let e = 1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t);
    e * std::f32::consts::FRAC_PI_2
}

/// Per-frame chest pass: advance the lid animations + draw every
/// loaded chest. `chests` is the authoritative container map (its
/// keys are the chest positions), `facing` the per-chest facing,
/// `open_local` the locally-open chest UI, `viewers` the host-side
/// viewer registry (MP).
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_chests(
    r: &mut Renderer,
    meshes: &ChestMeshes,
    chests: &HashMap<[i32; 3], crate::container::Container>,
    facing: &HashMap<[i32; 3], u8>,
    open_local: Option<[i32; 3]>,
    viewers: &HashMap<[i32; 3], u32>,
    lid_anim: &mut HashMap<[i32; 3], f32>,
    player_pos: Vec3,
    dt: f32,
    // W172: vx.chests is now a shared container store (chests, barrels,
    // ender chests, hoppers all live there). Only CHEST_ID positions
    // render the chest entity model - the rest draw as their own cube.
    world: &infengine_voxel::World,
) {
    if !meshes.loaded() {
        return;
    }
    let is_chest = |p: [i32; 3]| world.get(p).0 == crate::block::CHEST_ID;
    const DRAW_RANGE_SQ: f32 = 96.0 * 96.0;
    // MC's lid moves 0.1 per tick = full swing in ~0.5 s.
    let step = dt / 0.5;
    let is_open = |p: [i32; 3]| -> bool {
        open_local == Some(p) || viewers.contains_key(&p)
    };
    for (&pos, _) in chests.iter() {
        // W172: skip barrels / ender chests / hoppers - they share the
        // store but render as their own cube, not a chest entity.
        if !is_chest(pos) {
            continue;
        }
        let center = Vec3::new(
            pos[0] as f32 + 0.5,
            pos[1] as f32,
            pos[2] as f32 + 0.5,
        );
        if (center - player_pos).length_squared() > DRAW_RANGE_SQ {
            continue;
        }
        let f = facing.get(&pos).copied().unwrap_or(3);
        // Double-chest pairing: the adjacent cell along the
        // perpendicular axis with the SAME facing.
        let ext = ext_pos_dir(f);
        let pos_side = [pos[0] + ext[0], pos[1], pos[2] + ext[2]];
        let neg_side = [pos[0] - ext[0], pos[1], pos[2] - ext[2]];
        let pair_pos = chests.contains_key(&pos_side)
            && is_chest(pos_side)
            && facing.get(&pos_side).copied().unwrap_or(3) == f;
        let pair_neg = !pair_pos
            && chests.contains_key(&neg_side)
            && is_chest(neg_side)
            && facing.get(&neg_side).copied().unwrap_or(3) == f;
        // Lid state: a pair shares one lid - either half open lifts
        // both.
        let mut open = is_open(pos);
        if pair_pos {
            open = open || is_open(pos_side);
        }
        if pair_neg {
            open = open || is_open(neg_side);
        }
        let t = lid_anim.entry(pos).or_insert(0.0);
        if open {
            *t = (*t + step).min(1.0);
        } else {
            *t = (*t - step).max(0.0);
        }
        let angle = lid_angle(*t);
        let model = if pair_pos {
            meshes.ext_pos.as_ref()
        } else if pair_neg {
            meshes.ext_neg.as_ref()
        } else {
            meshes.single.as_ref()
        };
        let Some(baked) = model else { continue };
        entity_model::draw(
            r,
            baked,
            center,
            yaw_for_facing(f),
            [1.0, 1.0, 1.0, 1.0],
            |bone| {
                if bone == "chest_lid" {
                    BonePose {
                        extra_rotation: [-angle, 0.0, 0.0],
                        ..Default::default()
                    }
                } else {
                    BonePose::default()
                }
            },
        );
    }
    // Drop anim entries for chests that no longer exist so the map
    // doesn't grow across breaks.
    lid_anim.retain(|p, _| chests.contains_key(p));
}
