//! W47.13 - place / break decision helpers extracted from `on_update`.
//!
//! The block-edit path inside `Voxel::on_update` is a ~1100 LOC
//! sequence with deep `&mut self` borrows + side effects (chunk
//! rebuilds, edit broadcasts, particle spawns, inventory writes).
//! Extracting the entire flow at once is high-risk; this module
//! instead grows piece-by-piece with pure free functions that the
//! main loop calls. Each helper takes only the fields it needs so
//! the caller can keep other borrows alive.
//!
//! W47.13a: `resolve_attack_target` - given the three candidate hits
//! (remote player, mob, voxel), return whichever wins by closest
//! distance. Pure data; no engine dependencies.

/// Outcome of the per-frame combat-vs-block-edit arbitration.
///
/// At most one of `player` / `mob` is set; if `voxel_dist` was the
/// closest hit (or no candidate beat infinity) both are `None` and
/// the caller falls through to the block-break / place path.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AttackTarget {
    pub player: Option<u32>,
    pub mob: Option<u32>,
}

/// Pick whichever attack target is closest: player < mob < voxel.
///
/// The original main.rs sequence walked the three candidates in
/// (pvp, mob, voxel) order, each guarded by `d < best_dist`. The
/// voxel branch only resets the two combat outputs - it never sets
/// them - because a closer voxel means "let the block-edit path
/// handle it" rather than "attack the voxel".
pub fn resolve_attack_target(
    pvp_target: Option<(u32, f32)>,
    mob_target: Option<(u32, f32)>,
    voxel_dist: Option<f32>,
) -> AttackTarget {
    let mut best = f32::INFINITY;
    let mut player: Option<u32> = None;
    let mut mob: Option<u32> = None;
    if let Some((id, d)) = pvp_target
        && d < best
    {
        best = d;
        player = Some(id);
        mob = None;
    }
    if let Some((id, d)) = mob_target
        && d < best
    {
        best = d;
        mob = Some(id);
        player = None;
    }
    if let Some(d) = voxel_dist
        && d < best
    {
        mob = None;
        player = None;
    }
    AttackTarget { player, mob }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_candidates_returns_empty() {
        let r = resolve_attack_target(None, None, None);
        assert_eq!(r, AttackTarget::default());
    }

    #[test]
    fn voxel_only_blocks_combat() {
        let r = resolve_attack_target(None, None, Some(3.0));
        assert_eq!(r, AttackTarget::default());
    }

    #[test]
    fn pvp_wins_when_closest() {
        let r = resolve_attack_target(
            Some((7, 1.0)),
            Some((9, 2.0)),
            Some(3.0),
        );
        assert_eq!(r, AttackTarget { player: Some(7), mob: None });
    }

    #[test]
    fn mob_wins_when_closer_than_player() {
        let r = resolve_attack_target(
            Some((7, 4.0)),
            Some((9, 2.0)),
            Some(3.0),
        );
        assert_eq!(r, AttackTarget { player: None, mob: Some(9) });
    }

    #[test]
    fn voxel_closest_suppresses_pvp_and_mob() {
        let r = resolve_attack_target(
            Some((7, 4.0)),
            Some((9, 5.0)),
            Some(1.0),
        );
        assert_eq!(r, AttackTarget::default());
    }

    #[test]
    fn mob_only_when_no_pvp_and_no_voxel() {
        let r = resolve_attack_target(None, Some((9, 2.0)), None);
        assert_eq!(r, AttackTarget { player: None, mob: Some(9) });
    }
}
