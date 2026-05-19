//! Per-world + per-player game mode (W8.2).
//!
//! The enum is added in Wave 8 and persisted on both `WorldJson`
//! (the world's default mode for new joiners) + `PlayerSave`
//! (the player's current mode, may diverge from the default if
//! the user toggles in-game via Wave 10's slash command).
//!
//! For Wave 8 the mode is stored only - the runtime checks land
//! in Wave 10 (fly / invuln / insta-break / creative tab). Each
//! consumer site is tagged with a `TODO(W10):` comment.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// Game mode discriminant. Hardcore is deferred to a future wave
/// per the W8 spec. `Default` is wired via `#[default]` on the
/// Survival variant - matches the W8.2 spec ("new worlds default
/// to Survival").
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum GameMode {
    /// HP + damage + breaking takes time + can starve. The default
    /// for new worlds.
    #[default]
    Survival,
    /// Fly + invuln + instant-break + access to the creative
    /// inventory tab.
    Creative,
    // Hardcore: deferred to a future wave.
}

impl GameMode {
    /// Persistent string tag - written into `world.json` +
    /// `player.bin`. Stable across format-version bumps so future
    /// migrations don't have to chase enum-discriminant changes.
    pub(crate) fn as_tag(self) -> &'static str {
        match self {
            Self::Survival => "survival",
            Self::Creative => "creative",
        }
    }

    /// Inverse of `as_tag`. Unknown strings round to `Survival` so
    /// a corrupted / future tag never deserialises into nonsense.
    pub(crate) fn from_tag(tag: &str) -> Self {
        match tag {
            "creative" => Self::Creative,
            _ => Self::Survival,
        }
    }

    /// Human-readable Czech label for the menu dropdown.
    pub(crate) fn label_cs(self) -> &'static str {
        match self {
            Self::Survival => "Survival",
            Self::Creative => "Creative",
        }
    }

    /// Convenience predicate - true only in Creative. Mirrors
    /// MC's `GameType::isCreative()` in `GameType.java`.
    pub(crate) fn is_creative(self) -> bool {
        matches!(self, Self::Creative)
    }

    /// True in any mode where the player can take damage,
    /// crafts have hunger drain, etc. Mirrors MC's
    /// `GameType::isSurvival()` in `GameType.java`.
    pub(crate) fn is_survival(self) -> bool {
        matches!(self, Self::Survival)
    }
}

/// Per-player capability bundle derived from the current
/// `GameMode`. Mirrors MC's `net.minecraft.world.entity.player.Abilities`
/// struct in `Abilities.java` lines 14-44, with the same flag
/// names so cross-referencing is straightforward.
///
/// The bundle is recomputed every time `voxel.game_mode`
/// changes (world load, `/gamemode` command). Consumers read
/// the cached flags instead of branching on `game_mode`
/// directly so future modes (Adventure, Spectator) can land
/// without touching every call site.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Abilities {
    /// Player may toggle the fly state. Creative=true.
    pub mayfly: bool,
    /// Player is currently flying. Toggled by double-tap Space
    /// when `mayfly` is true. Off by default.
    pub flying: bool,
    /// Block breaking is instant (no mining timer). Creative=true.
    pub instabuild: bool,
    /// Player is immune to non-Void damage. Creative=true.
    pub invulnerable: bool,
    /// Player can place and break blocks at all. Reserved for
    /// future Adventure mode; today every mode opts in.
    pub may_build: bool,
    /// Base flight speed multiplier. Matches MC's default of
    /// 0.05 ("flying_speed" in Abilities.java).
    pub fly_speed: f32,
    /// Base walk speed multiplier. Matches MC's default of 0.1
    /// ("walking_speed" in Abilities.java).
    pub walk_speed: f32,
}

impl Abilities {
    /// Map a `GameMode` to its default ability bundle. The
    /// numeric defaults match MC's `Abilities.java` constants.
    pub(crate) fn for_mode(mode: GameMode) -> Self {
        match mode {
            GameMode::Survival => Self {
                mayfly: false,
                flying: false,
                instabuild: false,
                invulnerable: false,
                may_build: true,
                fly_speed: 0.05,
                walk_speed: 0.1,
            },
            GameMode::Creative => Self {
                mayfly: true,
                // Creative starts grounded; the player toggles
                // flight in-game via double-tap Space. MC
                // matches: `Abilities::flying` defaults to false
                // even when `mayfly = true`.
                flying: false,
                instabuild: true,
                invulnerable: true,
                may_build: true,
                fly_speed: 0.05,
                walk_speed: 0.1,
            },
        }
    }
}

impl Default for Abilities {
    /// Survival defaults so `Abilities::default()` is safe to
    /// use in unit tests + struct initialisation.
    fn default() -> Self {
        Self::for_mode(GameMode::Survival)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_survival() {
        // W8.2 spec: new worlds default to Survival; the Hardcore
        // variant lands in a future wave.
        assert_eq!(GameMode::default(), GameMode::Survival);
    }

    #[test]
    fn tag_round_trips_through_strings() {
        for m in [GameMode::Survival, GameMode::Creative] {
            let tag = m.as_tag();
            assert_eq!(GameMode::from_tag(tag), m);
        }
    }

    #[test]
    fn unknown_tag_falls_back_to_survival() {
        // Defensive: a corrupted / forward-compatible tag must
        // never panic + must produce a safe default.
        assert_eq!(GameMode::from_tag("hardcore"), GameMode::Survival);
        assert_eq!(GameMode::from_tag(""), GameMode::Survival);
        assert_eq!(GameMode::from_tag("unknown"), GameMode::Survival);
    }

    #[test]
    fn labels_are_non_empty() {
        // The menu dropdown shows the label; an empty string would
        // collapse the row width.
        for m in [GameMode::Survival, GameMode::Creative] {
            assert!(!m.label_cs().is_empty());
        }
    }

    #[test]
    fn survival_abilities_match_mc() {
        // MC Abilities.java lines 14-44 + Player::isSurvival.
        // Survival bundle: no fly toggle, no instant-build, no
        // invulnerability; building stays enabled.
        let a = Abilities::for_mode(GameMode::Survival);
        assert!(!a.mayfly);
        assert!(!a.flying);
        assert!(!a.instabuild);
        assert!(!a.invulnerable);
        assert!(a.may_build);
        // MC defaults for fly_speed / walk_speed.
        assert!((a.fly_speed - 0.05).abs() < 1e-6);
        assert!((a.walk_speed - 0.1).abs() < 1e-6);
    }

    #[test]
    fn creative_abilities_match_mc() {
        // MC Abilities.java: Creative gets mayfly + instabuild +
        // invulnerable. `flying` stays false until the player
        // toggles it in-game (matches MC's LocalPlayer init).
        let a = Abilities::for_mode(GameMode::Creative);
        assert!(a.mayfly);
        assert!(!a.flying);
        assert!(a.instabuild);
        assert!(a.invulnerable);
        assert!(a.may_build);
    }

    #[test]
    fn abilities_default_is_survival() {
        // Sanity: `Abilities::default()` should not silently
        // grant fly/invuln to a freshly-constructed slot.
        let a = Abilities::default();
        let s = Abilities::for_mode(GameMode::Survival);
        assert_eq!(a, s);
    }

    #[test]
    fn is_creative_and_is_survival_are_exhaustive() {
        // Sanity: every variant must round-trip through exactly
        // one of the two predicates. This catches a future
        // Adventure / Spectator addition that forgets to update
        // both helpers.
        for m in [GameMode::Survival, GameMode::Creative] {
            assert_ne!(m.is_creative(), m.is_survival());
        }
    }
}
