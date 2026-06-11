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
use infengine_render::Renderer;
use std::collections::HashMap;

use crate::entity_model::{self, BakedModel, BonePose};

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
        out.single = Some(entity_model::bake(r, &entity_model::chest::MODEL, &img));
    }
    // Sheet pairing derives from the transparent SEAM tile: the MC
    // box unwrap maps the "right" tile to the model +X face;
    // normal_right.png leaves its "left" (-X) tile transparent, so
    // it belongs to the half whose seam sits at model -X = EXT_POS
    // (the flip negates X: world +X extension = model -X seam).
    if let Some(img) = load("normal_right.png") {
        out.ext_pos = Some(entity_model::bake(r, &entity_model::chest::MODEL_EXT_POS, &img));
    }
    if let Some(img) = load("normal_left.png") {
        out.ext_neg = Some(entity_model::bake(r, &entity_model::chest::MODEL_EXT_NEG, &img));
    }
    tracing::info!(loaded = out.loaded(), "chest entity meshes baked");
    out
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
) {
    if !meshes.loaded() {
        return;
    }
    const DRAW_RANGE_SQ: f32 = 96.0 * 96.0;
    // MC's lid moves 0.1 per tick = full swing in ~0.5 s.
    let step = dt / 0.5;
    let is_open = |p: [i32; 3]| -> bool {
        open_local == Some(p) || viewers.contains_key(&p)
    };
    for (&pos, _) in chests.iter() {
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
            && facing.get(&pos_side).copied().unwrap_or(3) == f;
        let pair_neg = !pair_pos
            && chests.contains_key(&neg_side)
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
                    BonePose { extra_rotation: [-angle, 0.0, 0.0] }
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
