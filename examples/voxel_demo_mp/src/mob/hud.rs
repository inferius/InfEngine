//! F3 debug-overlay helpers for the mob subsystem.
//!
//! Produces the small `Mobs: <n> / <cap> (nearest: <kind> @ <dist>m)`
//! line + a per-kind breakdown for the expanded debug overlay. Pure
//! string builder; the egui call site reads the result.
//!
//! Cited MC: `net/minecraft/client/gui/components/DebugScreenOverlay.java`
//! - the same pattern of "render a few lines on top of the gameplay
//!   HUD" used by MC's F3 panel. We adopt the format flavour without
//!   the full MC profiler dump.

use crate::Voxel;

use super::kinds::{
    KIND_CHICKEN, KIND_COW, KIND_PIG, KIND_SHEEP, KIND_SKELETON, KIND_SPIDER, KIND_ZOMBIE,
};

/// Format the mob count + nearest-mob line for the F3 overlay.
/// Pure: pulls from `Voxel.mobs` + `player_pos` + `mob_cap`. Returns
/// `Mobs: <count> / <cap>` on no mobs, `Mobs: <count> / <cap>
/// (nearest: <kind> @ <dist>m)` otherwise.
pub(crate) fn format_mob_count_line(voxel: &Voxel) -> String {
    let count = voxel.mobs.len();
    let cap = voxel.mob_cap;
    let nearest = nearest_mob(voxel);
    match nearest {
        Some((kind, d)) => format!(
            "mobs: {count} / {cap} (nearest: {} @ {d:.1}m)",
            kind_label(kind)
        ),
        None => format!("mobs: {count} / {cap}"),
    }
}

/// Format the per-kind breakdown line: `Mob kinds: 3 sheep, 2 cow,
/// 1 zombie`. Empty when there are no mobs at all.
pub(crate) fn format_mob_kinds_line(voxel: &Voxel) -> String {
    let counts = per_kind_counts(voxel);
    if counts.iter().all(|(_, n)| *n == 0) {
        return String::new();
    }
    let parts: Vec<String> = counts
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(k, n)| format!("{n} {}", kind_label(*k)))
        .collect();
    format!("kinds: {}", parts.join(", "))
}

/// Find the mob nearest to the player. Returns `(kind, distance)`.
/// `None` when no mobs exist.
pub(crate) fn nearest_mob(voxel: &Voxel) -> Option<(u8, f32)> {
    let p = voxel.player_pos;
    let mut best: Option<(u8, f32)> = None;
    for m in &voxel.mobs {
        let dx = m.pos[0] - p.x;
        let dy = m.pos[1] - p.y;
        let dz = m.pos[2] - p.z;
        let d = (dx * dx + dy * dy + dz * dz).sqrt();
        match best {
            Some((_, bd)) if bd <= d => {}
            _ => best = Some((m.kind, d)),
        }
    }
    best
}

/// Tally up counts per `MobKind` discriminator. Returns a fixed-
/// shape array so callers can iterate it without a HashMap walk.
pub(crate) fn per_kind_counts(voxel: &Voxel) -> [(u8, u32); 7] {
    let mut counts: [(u8, u32); 7] = [
        (KIND_SHEEP, 0),
        (KIND_ZOMBIE, 0),
        (KIND_CHICKEN, 0),
        (KIND_PIG, 0),
        (KIND_COW, 0),
        (KIND_SKELETON, 0),
        (KIND_SPIDER, 0),
    ];
    for m in &voxel.mobs {
        for entry in counts.iter_mut() {
            if entry.0 == m.kind {
                entry.1 += 1;
                break;
            }
        }
    }
    counts
}

/// Short lowercase kind label. Centralised here so the F3 panel
/// can rename a kind by editing one table.
fn kind_label(kind: u8) -> &'static str {
    match kind {
        KIND_SHEEP => "sheep",
        KIND_ZOMBIE => "zombie",
        KIND_CHICKEN => "chicken",
        KIND_PIG => "pig",
        KIND_COW => "cow",
        KIND_SKELETON => "skeleton",
        KIND_SPIDER => "spider",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    //! HUD format regressions. Build a `Voxel` with hand-picked
    //! mob lists + assert the string output.

    use super::*;
    use super::super::state::MobState;
    use crate::Voxel;

    fn push_mob(v: &mut Voxel, id: u32, kind: u8, pos: [f32; 3]) {
        v.mobs.push(MobState::new_natural(
            id,
            kind,
            pos,
            pos,
            0.0,
            0.0,
            8.0,
        ));
    }

    #[test]
    fn format_count_line_zero_mobs() {
        let v = Voxel::new();
        let s = format_mob_count_line(&v);
        assert!(s.starts_with("mobs: 0 /"), "{}", s);
        assert!(!s.contains("nearest"), "{}", s);
    }

    #[test]
    fn format_count_line_includes_nearest_kind() {
        let mut v = Voxel::new();
        // Voxel::new() puts the player at y=32, so place mobs at
        // matching y to keep distance math simple.
        let py = v.player_pos.y;
        push_mob(&mut v, 1, KIND_SHEEP, [3.0, py, 0.0]);
        push_mob(&mut v, 2, KIND_ZOMBIE, [10.0, py, 0.0]);
        let s = format_mob_count_line(&v);
        // Nearest is sheep at distance 3.
        assert!(s.contains("sheep"), "{}", s);
        assert!(s.contains("3.0m"), "{}", s);
    }

    #[test]
    fn format_count_line_picks_strictly_closer() {
        let mut v = Voxel::new();
        let py = v.player_pos.y;
        push_mob(&mut v, 1, KIND_SHEEP, [10.0, py, 0.0]);
        push_mob(&mut v, 2, KIND_ZOMBIE, [5.0, py, 0.0]);
        let s = format_mob_count_line(&v);
        assert!(s.contains("zombie"), "{}", s);
    }

    #[test]
    fn format_kinds_line_empty_when_no_mobs() {
        let v = Voxel::new();
        assert!(format_mob_kinds_line(&v).is_empty());
    }

    #[test]
    fn format_kinds_line_lists_each_kind_present() {
        let mut v = Voxel::new();
        push_mob(&mut v, 1, KIND_SHEEP, [0.0, 0.0, 0.0]);
        push_mob(&mut v, 2, KIND_SHEEP, [1.0, 0.0, 0.0]);
        push_mob(&mut v, 3, KIND_ZOMBIE, [2.0, 0.0, 0.0]);
        let s = format_mob_kinds_line(&v);
        assert!(s.contains("2 sheep"), "{}", s);
        assert!(s.contains("1 zombie"), "{}", s);
    }

    #[test]
    fn per_kind_counts_partition_total() {
        let mut v = Voxel::new();
        push_mob(&mut v, 1, KIND_SHEEP, [0.0, 0.0, 0.0]);
        push_mob(&mut v, 2, KIND_SHEEP, [0.0, 0.0, 0.0]);
        push_mob(&mut v, 3, KIND_PIG, [0.0, 0.0, 0.0]);
        let counts = per_kind_counts(&v);
        let total: u32 = counts.iter().map(|(_, n)| *n).sum();
        assert_eq!(total, 3);
    }

    #[test]
    fn nearest_mob_none_when_empty() {
        let v = Voxel::new();
        assert!(nearest_mob(&v).is_none());
    }

    #[test]
    fn nearest_mob_returns_kind_and_distance() {
        let mut v = Voxel::new();
        let py = v.player_pos.y;
        push_mob(&mut v, 1, KIND_COW, [4.0, py, 3.0]);
        let (k, d) = nearest_mob(&v).unwrap();
        assert_eq!(k, KIND_COW);
        // sqrt(16+9) = 5
        assert!((d - 5.0).abs() < 0.01);
    }

    #[test]
    fn kind_label_table_round_trips_every_known_kind() {
        for k in [
            KIND_SHEEP,
            KIND_ZOMBIE,
            KIND_CHICKEN,
            KIND_PIG,
            KIND_COW,
            KIND_SKELETON,
            KIND_SPIDER,
        ] {
            let s = kind_label(k);
            assert!(s.is_ascii());
            assert_ne!(s, "unknown");
        }
    }

    #[test]
    fn kind_label_unknown_kind_returns_unknown() {
        assert_eq!(kind_label(200), "unknown");
    }
}
