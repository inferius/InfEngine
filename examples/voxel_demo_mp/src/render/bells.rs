//! W400 - bell block entity (MC `BellRenderer`).
//!
//! The stand (posts + crossbar from `bell_floor.json`) lives in the
//! chunk mesh - `emit_bell_stand` - while the swinging body draws
//! here per frame with the real `entity/bell/bell.png` sheet,
//! exactly MC's split. Cells are indexed by `Voxel::bell_positions`
//! (worker gen side effect + the emitter rescan on load). The
//! on-ring swing animation is a follow-up.

use glam::Vec3;
use infengine_render::Renderer;

use crate::entity_model::{self, BakedModel, BonePose};

pub(crate) fn load_bell_mesh(
    r: &mut Renderer,
    entity_dir: Option<&std::path::Path>,
) -> Option<BakedModel> {
    let dir = entity_dir?;
    let p = dir.join("bell").join("bell.png");
    match image::open(&p) {
        Ok(img) => {
            let img = img.to_rgba8();
            let baked =
                entity_model::bake(r, &crate::entity_model::bell::MODEL, &img);
            tracing::info!("bell entity mesh baked");
            Some(baked)
        }
        Err(err) => {
            tracing::warn!(error = %err, path = %p.display(), "bell sheet load failed");
            None
        }
    }
}

pub(crate) fn draw_bells(
    r: &mut Renderer,
    mesh: &Option<BakedModel>,
    positions: &std::collections::HashSet<[i32; 3]>,
    player_pos: Vec3,
) {
    let Some(baked) = mesh else {
        return;
    };
    const DRAW_RANGE_SQ: f32 = 96.0 * 96.0;
    for pos in positions {
        let world = Vec3::new(
            pos[0] as f32 + 0.5,
            pos[1] as f32,
            pos[2] as f32 + 0.5,
        );
        if (world - player_pos).length_squared() > DRAW_RANGE_SQ {
            continue;
        }
        entity_model::draw(r, baked, world, 0.0, [1.0, 1.0, 1.0, 1.0], |_| {
            BonePose::default()
        });
    }
}
