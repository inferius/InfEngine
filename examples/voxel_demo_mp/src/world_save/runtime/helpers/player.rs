//! Player pose + inventory save / apply. Builds `PlayerSave` from
//! the live host state and writes it back on load.

use infengine_voxel::BlockId;

use crate::ui::HighlightMode;
use crate::{DAY_LENGTH_S, PLAYER_MAX_HEALTH, Voxel};

use crate::world_save::players::{
    DEFAULT_HOST_PERMISSION, PLAYER_FORMAT_VERSION, PlayerSave,
};

/// Build a `PlayerSave` from the live host pose + inventory.
pub(in crate::world_save::runtime) fn build_player_save(voxel: &Voxel) -> PlayerSave {
    // W466: inventory slots persist as full `ItemStack`s (id +
    // count + components) - a straight clone, no tuple projection.
    let inventory: Vec<Option<crate::item::ItemStack>> =
        voxel.inventory.to_vec();
    // The runtime craft grid is 2x2 (4 slots) but the save schema
    // budgets a 3x3 grid (9 slots) so Phase D's larger grid lands
    // without bumping the format. Pad the unused slots with `None`.
    let mut craft: Vec<Option<(u16, u8)>> = vec![None; 9];
    for (i, slot) in voxel.craft_grid.iter().enumerate() {
        if i >= craft.len() {
            break;
        }
        if let Some((b, c)) = slot {
            craft[i] = Some((b.0, *c));
        }
    }
    PlayerSave {
        format_version: PLAYER_FORMAT_VERSION,
        pos: [voxel.player_pos.x, voxel.player_pos.y, voxel.player_pos.z],
        yaw: voxel.player_yaw,
        pitch: voxel.player_pitch,
        inventory,
        craft_grid: craft,
        selected_slot: voxel.selected_slot.min(u8::MAX as usize) as u8,
        air: voxel.player_air,
        health: voxel.player_health,
        home_pos: voxel.home_pos,
        spawn_pos: voxel.spawn_pos,
        permission_level: DEFAULT_HOST_PERMISSION,
        play_time_seconds: 0,
        // Phase F: the active dim name is the source of truth on
        // the runtime; the legacy single-dim path always wrote
        // OVERWORLD here, which broke cross-dim save / load.
        last_dimension: voxel.current_dim.clone(),
        highlight_mode: voxel.highlight_mode.to_u8(),
        // W8.2: per-player game mode. The runtime field is the
        // source of truth; the in-game toggle path lands in
        // Wave 10 (Survival / Creative semantics).
        game_mode: voxel.game_mode as u8,
        // W416: hunger pair.
        food: voxel.player_food.food,
        saturation: voxel.player_food.saturation,
        // W418: XP total.
        xp: voxel.player_xp,
        // W419: worn armor.
        armor: voxel.armor_slots.to_vec(),
    }
}

/// Apply a `PlayerSave` back onto `voxel`. Resets pose, health,
/// inventory, etc. The day-clock + kill counter live in the world
/// payload, not the player file - those stay untouched here.
pub(in crate::world_save::runtime) fn apply_player_save(voxel: &mut Voxel, save: &PlayerSave) {
    voxel.player_pos = glam::Vec3::new(save.pos[0], save.pos[1], save.pos[2]);
    voxel.player_yaw = save.yaw;
    voxel.player_pitch = save.pitch;
    for (i, slot) in save.inventory.iter().enumerate() {
        if i >= voxel.inventory.len() {
            break;
        }
        // W466: slots round-trip as full `ItemStack`s - damage +
        // future components survive the save/load hop.
        voxel.inventory[i] = slot.clone();
    }
    // Craft grid is 2x2 in the runtime, 3x3 in the save - only the
    // first 4 slots round-trip.
    for (i, slot) in save.craft_grid.iter().enumerate() {
        if i >= voxel.craft_grid.len() {
            break;
        }
        voxel.craft_grid[i] = slot.map(|(b, c)| (BlockId(b), c));
    }
    voxel.selected_slot = (save.selected_slot as usize).min(voxel.inventory.len() - 1);
    voxel.player_air = save.air;
    voxel.player_health = save.health.clamp(0.0, PLAYER_MAX_HEALTH);
    voxel.home_pos = save.home_pos;
    voxel.spawn_pos = save.spawn_pos;
    voxel.highlight_mode = HighlightMode::from_u8(save.highlight_mode);
    // W8.2: load the saved game mode. Wave 10 wires the
    // fly/invuln/insta-break flips against this field.
    voxel.game_mode = match save.game_mode {
        1 => crate::game_mode::GameMode::Creative,
        _ => crate::game_mode::GameMode::Survival,
    };
    // W416: hunger pair (clamped like MC load; exhaustion is
    // transient and restarts at 0).
    voxel.player_food.food = save.food.clamp(0.0, 20.0);
    voxel.player_food.saturation =
        save.saturation.clamp(0.0, voxel.player_food.food);
    voxel.player_food.exhaustion = 0.0;
    // W418: XP total.
    voxel.player_xp = save.xp;
    // W419: worn armor (tolerates short vecs from hand-edited
    // files).
    voxel.armor_slots = [None; 4];
    for (i, slot) in save.armor.iter().take(4).enumerate() {
        voxel.armor_slots[i] = *slot;
    }
    // W10.1: keep the cached ability bundle in lockstep with
    // `game_mode`. Without this the fly / invuln / instabuild
    // flags would lag a tick behind the loaded mode.
    voxel.refresh_abilities();
    // Re-anchor the day clock so a load right after a save doesn't
    // jump the sun. `day_time` itself lives in `meta.json` once Phase
    // C lands; for now the runtime keeps its value across reloads.
    voxel.day_time = voxel.day_time.rem_euclid(DAY_LENGTH_S);
}
