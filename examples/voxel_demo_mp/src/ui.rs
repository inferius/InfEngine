//! UI helper types + crafting recipe display lookup.
//!
//! The actual egui draw closure (main menu, world picker, MP
//! setup, pause, settings, F6 debug, HUD, inventory grid,
//! chat overlay) stays inline in `Voxel::update_play` for now.
//! That closure captures 40+ mutable + immutable references
//! to individual `Voxel` fields and shadow locals in the same
//! scope, so extracting it as a free fn would either
//! materialise a function with that many parameters or force
//! an aggregate `UiContext` struct that gets wired up
//! immediately before the closure - either way without
//! making the call site more readable. Tracked as a follow-up
//! once the surrounding play-update orchestration is itself
//! a leaner shell.
//!
//! What this module DOES own: the small UI-state types
//! (`SlotIcon`, `InvAction`, `AppState`, `BackTo`) that get
//! handed to / produced by the egui closure, and the
//! recipe-matching lookup that drives the craft grid's output
//! display + tooltip.

use infengine_voxel::BlockId;
use serde::{Deserialize, Serialize};

use crate::inventory::{SlotKind, SplitMode};
use crate::RECIPES;

/// Targeting wireframe mode, cycled by the `H` key.
///
/// Drives the per-frame highlight box drawn around the voxel the
/// crosshair is on:
///
/// - `Auto`: if the held item resolves to a block (placeable), the
///   box previews the place-target cell; otherwise it falls back to
///   the break wireframe around the targeted voxel. Same behaviour
///   the demo shipped with before Q.2.
/// - `Build`: always draws the place-target preview, even when the
///   held item is a tool / has no block id. Useful when planning a
///   structure with a sword in hand.
/// - `Break`: always draws the break wireframe around the targeted
///   voxel, even when a placeable block is held. Useful for fast
///   demolition without an empty slot selected.
///
/// Persisted per-player in `PlayerSave::highlight_mode` (Phase Q.2
/// migration; legacy v1 saves default to `Auto`).
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum HighlightMode {
    /// Behaviour-driven: pick build or break based on the held
    /// item. Default on a fresh save.
    #[default]
    Auto,
    /// Always show the build (place-target) preview.
    Build,
    /// Always show the break wireframe at the targeted voxel.
    Break,
}

impl HighlightMode {
    /// Next mode in the `Auto -> Build -> Break -> Auto` cycle.
    pub(crate) fn cycle(self) -> Self {
        match self {
            HighlightMode::Auto => HighlightMode::Build,
            HighlightMode::Build => HighlightMode::Break,
            HighlightMode::Break => HighlightMode::Auto,
        }
    }

    /// Short user-facing label for HUD toast lines.
    pub(crate) fn label(self) -> &'static str {
        match self {
            HighlightMode::Auto => "Auto",
            HighlightMode::Build => "Build",
            HighlightMode::Break => "Break",
        }
    }

    /// Wire / on-disk byte representation. Stable across builds so
    /// PlayerSave migrations can rely on the mapping.
    pub(crate) fn to_u8(self) -> u8 {
        match self {
            HighlightMode::Auto => 0,
            HighlightMode::Build => 1,
            HighlightMode::Break => 2,
        }
    }

    /// Inverse of `to_u8`. Unknown bytes round to `Auto` so a future
    /// schema bump can read older / unexpected files without
    /// blowing up.
    pub(crate) fn from_u8(b: u8) -> Self {
        match b {
            1 => HighlightMode::Build,
            2 => HighlightMode::Break,
            _ => HighlightMode::Auto,
        }
    }
}

/// Visual snapshot of one slot, lifted out of the inventory
/// state for the egui pass. `(color, count, uv_rect, name)`
/// where `uv_rect` is the atlas tile to paint (we always show
/// the top face) and `name` is the block's display name used
/// for the hover tooltip.
pub(crate) type SlotIcon = ([f32; 3], u8, [f32; 4], &'static str);

/// UI intent captured during the egui pass + applied after the
/// closure exits. Drag operations are the main interaction;
/// the hotbar still supports a plain click to change the
/// active slot.
#[derive(Debug, Clone, Copy)]
pub(crate) enum InvAction {
    /// Plain click on a hotbar slot: changes the active
    /// selection.
    SelectHotbar(usize),
    /// Drag&drop finished: move stack from `from` to `to`. The
    /// `split` field determines how much of the source stack
    /// transfers - `All` is the standard whole-stack move,
    /// `Half` shifts the upper half (good for splitting),
    /// `One` peels off a single item (good for filling craft
    /// slots).
    Drag {
        from: SlotKind,
        to: SlotKind,
        split: SplitMode,
    },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum AppState {
    MainMenu,
    /// Direct-IP connect dialog: user types `host:port` and
    /// hits Join.
    DirectConnect,
    /// Browses servers fetched from the configured master
    /// server.
    ServerBrowser,
    /// Configures + spawns a host (game server). User can opt
    /// to announce on the master so others see it in the
    /// browser.
    HostSetup,
    Playing,
    Paused,
    Settings {
        back_to: BackTo,
    },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub(crate) enum BackTo {
    MainMenu,
    Pause,
}

/// Inspect the 2x2 crafting grid and return the recipe output
/// if every input is satisfied. The grid is shapeless: order
/// doesn't matter. Returns `None` when no recipe matches or
/// any required stack is short.
pub(crate) fn match_recipe(grid: &[Option<(BlockId, u8)>; 4]) -> Option<(BlockId, u8)> {
    match_recipe_named(grid).map(|(b, c, _)| (b, c))
}

/// Same as `match_recipe` but also returns the matching
/// recipe's human-readable name. Used by the UI to display
/// the result name as a tooltip on the output slot.
pub(crate) fn match_recipe_named(
    grid: &[Option<(BlockId, u8)>; 4],
) -> Option<(BlockId, u8, &'static str)> {
    // Aggregate the grid into a (block, count) tally so order
    // doesn't matter for shapeless matching.
    let mut tally: Vec<(u16, u8)> = Vec::with_capacity(4);
    for (b, c) in grid.iter().flatten() {
        if let Some(slot) = tally.iter_mut().find(|(bb, _)| *bb == b.0) {
            slot.1 = slot.1.saturating_add(*c);
        } else {
            tally.push((b.0, *c));
        }
    }
    for recipe in RECIPES {
        // Aggregate the recipe inputs the same way.
        let mut needed: Vec<(u16, u8)> = Vec::with_capacity(recipe.inputs.len());
        for (b, c) in recipe.inputs {
            if let Some(slot) = needed.iter_mut().find(|(bb, _)| *bb == *b) {
                slot.1 = slot.1.saturating_add(*c);
            } else {
                needed.push((*b, *c));
            }
        }
        // The match requires the tally to have at least every
        // (block, count) the recipe needs - and the tally must
        // contain no other blocks (otherwise the user just
        // stuffed random extras in).
        if needed.len() != tally.len() {
            continue;
        }
        let mut ok = true;
        for (b, n) in &needed {
            match tally.iter().find(|(bb, _)| bb == b) {
                Some((_, have)) if *have >= *n => {}
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            return Some((BlockId(recipe.output.0), recipe.output.1, recipe.name));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highlight_mode_default_is_auto() {
        assert_eq!(HighlightMode::default(), HighlightMode::Auto);
    }

    #[test]
    fn highlight_mode_cycle_covers_all_three_states() {
        let mut m = HighlightMode::Auto;
        m = m.cycle();
        assert_eq!(m, HighlightMode::Build);
        m = m.cycle();
        assert_eq!(m, HighlightMode::Break);
        m = m.cycle();
        assert_eq!(m, HighlightMode::Auto);
    }

    #[test]
    fn highlight_mode_to_from_u8_roundtrip() {
        for mode in [HighlightMode::Auto, HighlightMode::Build, HighlightMode::Break] {
            assert_eq!(HighlightMode::from_u8(mode.to_u8()), mode);
        }
    }

    #[test]
    fn highlight_mode_from_unknown_u8_falls_back_to_auto() {
        // Forward-compat: a future enum value should not break
        // older builds - round to Auto rather than panic / error.
        for b in [3u8, 7, 42, 255] {
            assert_eq!(HighlightMode::from_u8(b), HighlightMode::Auto);
        }
    }

    #[test]
    fn highlight_mode_labels_are_short_user_strings() {
        assert_eq!(HighlightMode::Auto.label(), "Auto");
        assert_eq!(HighlightMode::Build.label(), "Build");
        assert_eq!(HighlightMode::Break.label(), "Break");
    }
}
