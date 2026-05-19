//! `/dim` chat command (Phase F.7).
//!
//! Usage:
//!   `/dim`            - print the currently-active dim.
//!   `/dim list`       - print every known dim from world.json.
//!   `/dim <name>`     - admin-teleport to the named dim (no
//!                       coord scaling - drops the player onto
//!                       a safe spawn at the world's default
//!                       spawn coord). Requires permission
//!                       level 3+.
//!
//! Drops the player at a clamped Y so they don't fall into the
//! void on first switch. Doesn't search for an existing portal -
//! it's a debug / admin shortcut, not the gameplay teleport.

use crate::Voxel;
use crate::block::portal;

/// `/dim [list|<name>]`. Permission gating happens in the
/// dispatch table (`chat::perms::CMD_META`); this fn assumes
/// the call passed.
pub(crate) fn cmd_dim(
    voxel: &mut Voxel,
    arg: &str,
    lines: &mut Vec<String>,
) -> bool {
    let arg = arg.trim();
    if arg.is_empty() {
        lines.push(format!("dim: '{}'", voxel.current_dim));
        return true;
    }
    if arg.eq_ignore_ascii_case("list") {
        let dims: Vec<String> = voxel
            .dim_worldgens
            .keys()
            .cloned()
            .collect();
        if dims.is_empty() {
            // Fallback: at least show the active dim.
            lines.push(format!("dim: '{}'", voxel.current_dim));
        } else {
            let mut sorted = dims;
            sorted.sort();
            lines.push(format!("Known dims: {}", sorted.join(", ")));
            lines.push(format!("Active dim: '{}'", voxel.current_dim));
        }
        return true;
    }
    // /dim <name>: jump to the named dim (or no-op if same).
    let target = arg.to_lowercase();
    if target == voxel.current_dim {
        lines.push(format!("Already in dim '{target}'."));
        return true;
    }
    // Allow any known dim, plus the canonical "nether" /
    // "overworld" pair so a Phase E save without a populated
    // dim_worldgens table can still admin-warp via this cmd.
    let known = voxel.dim_worldgens.contains_key(&target)
        || target == crate::dim::OVERWORLD
        || target == crate::dim::NETHER;
    if !known {
        lines.push(format!(
            "Unknown dim '{target}'. Try /dim list."
        ));
        return true;
    }
    let prev_dim = voxel.current_dim.clone();
    voxel.switch_dim(&target);
    // Pre-load the destination chunks at the player's current
    // pos so the ensure_chunks bake into the world map; the
    // player otherwise falls into ungenerated air.
    let pos = [
        voxel.player_pos.x.floor() as i32,
        voxel.player_pos.y.floor() as i32,
        voxel.player_pos.z.floor() as i32,
    ];
    portal::ensure_return_portal(&mut voxel.world, pos, None);
    voxel.portal_cooldown_ticks = portal::PORTAL_COOLDOWN_TICKS;
    voxel.portal_in_ticks = 0;
    lines.push(format!("Switched dim: '{prev_dim}' -> '{target}'"));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Voxel;
    use crate::dim::{NETHER, OVERWORLD};
    use crate::worldgen::WorldGen;

    #[test]
    fn cmd_dim_with_no_arg_prints_current() {
        let mut v = Voxel::new();
        v.current_dim = OVERWORLD.to_string();
        let mut lines: Vec<String> = Vec::new();
        let handled = cmd_dim(&mut v, "", &mut lines);
        assert!(handled);
        assert!(lines.iter().any(|l| l.contains("overworld")));
    }

    #[test]
    fn cmd_dim_list_shows_known_dims() {
        let mut v = Voxel::new();
        v.current_dim = OVERWORLD.to_string();
        v.dim_worldgens
            .insert(OVERWORLD.to_string(), WorldGen::Classic);
        v.dim_worldgens
            .insert(NETHER.to_string(), WorldGen::Nether);
        let mut lines: Vec<String> = Vec::new();
        cmd_dim(&mut v, "list", &mut lines);
        assert!(lines.iter().any(|l| l.contains("nether")));
        assert!(lines.iter().any(|l| l.contains("overworld")));
    }

    #[test]
    fn cmd_dim_rejects_unknown_name() {
        let mut v = Voxel::new();
        v.current_dim = OVERWORLD.to_string();
        let mut lines: Vec<String> = Vec::new();
        cmd_dim(&mut v, "void", &mut lines);
        assert!(lines.iter().any(|l| l.contains("Unknown dim")));
        assert_eq!(v.current_dim, OVERWORLD);
    }

    #[test]
    fn cmd_dim_idempotent_when_target_matches() {
        let mut v = Voxel::new();
        v.current_dim = OVERWORLD.to_string();
        let mut lines: Vec<String> = Vec::new();
        cmd_dim(&mut v, "overworld", &mut lines);
        assert!(lines.iter().any(|l| l.contains("Already")));
        assert_eq!(v.current_dim, OVERWORLD);
    }
}
