//! W47.16 - dropped items render, relocated from on_update.
//!
//! Each floating drop renders as a small cube (~0.3 voxel) bobbing
//! in place with a y-axis spin keyed off `drop.age`. Inside the
//! magnet attract radius the cube shrinks toward the player +
//! spins faster + leans along the player delta so the pickup
//! reads as a satisfying suck-in animation.

use glam::{Mat4, Vec3};
use infengine_render::{MeshHandle, Renderer};
use infengine_voxel::{BlockId, BlockRegistry};

use crate::drops::{DROP_ATTRACT_RADIUS, DropItem};
use crate::PLAYER_HEIGHT;

/// Draw every entry in `dropped_items` as a small bobbing cube.
///
/// `player_cube` is the cached cube mesh handle owned by the App
/// (passed in so this helper doesn't have to touch `Voxel`).
/// `registry` reads the block's albedo for the tint; missing entries
/// fall back to a tan default.
pub(crate) fn draw(
    renderer: &mut Renderer,
    player_cube: MeshHandle,
    dropped_items: &[DropItem],
    player_pos: Vec3,
    registry: &BlockRegistry,
) {
    if dropped_items.is_empty() {
        return;
    }
    let chest_y = player_pos.y - PLAYER_HEIGHT * 0.4;
    for drop in dropped_items.iter() {
        let bob = (drop.age * 2.4).sin() * 0.08;
        let block = BlockId(drop.block);
        let color = registry.get(block).map(|d| d.color).unwrap_or([0.75, 0.55, 0.30]);
        // Magnet attract: scale shrinks + spin speeds up + drop
        // leans along the player delta inside DROP_ATTRACT_RADIUS.
        let dx = player_pos.x - drop.pos[0];
        let dy = chest_y - drop.pos[1];
        let dz = player_pos.z - drop.pos[2];
        let dist = (dx * dx + dy * dy + dz * dz).sqrt();
        let attract = if dist < DROP_ATTRACT_RADIUS && dist > 1e-3 {
            (1.0 - dist / DROP_ATTRACT_RADIUS).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let base_scale = 0.28;
        let scale = base_scale * (1.0 - 0.7 * attract);
        let spin_rate = 1.4 + 6.0 * attract;
        let lean = 0.45 * attract;
        let render_pos = Vec3::new(
            drop.pos[0] + dx * lean,
            drop.pos[1] + 0.14 + bob + dy * lean,
            drop.pos[2] + dz * lean,
        );
        let m = Mat4::from_scale_rotation_translation(
            Vec3::splat(scale),
            glam::Quat::from_rotation_y(drop.age * spin_rate),
            render_pos,
        );
        renderer.draw_mesh_pbr(
            player_cube,
            m.to_cols_array(),
            [color[0], color[1], color[2], 1.0],
            0.0,
            0.7,
        );
    }
}
