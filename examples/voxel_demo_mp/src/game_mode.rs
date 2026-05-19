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
}
