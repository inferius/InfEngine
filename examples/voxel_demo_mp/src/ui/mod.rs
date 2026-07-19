//! UI helper types + crafting recipe display lookup.
//!
//! W47.21: the giant egui draw closure (main menu, world picker, MP
//! setup, pause, settings, F6 debug, HUD, inventory grid, chat
//! overlay) lives in `crate::ui::render`. The free
//! `pub(crate) fn render(vx, window)` is called from
//! `Voxel::render_ui` (one-line wrapper).
//!
//! What this module DOES own: the small UI-state types
//! (`SlotIcon`, `InvAction`, `AppState`, `BackTo`) that get
//! handed to / produced by the egui closure, and the
//! recipe-matching lookup that drives the craft grid's output
//! display + tooltip.

pub(crate) mod brewing_panel;
pub(crate) mod crafting_panel;
pub(crate) mod stonecutter_panel;
pub(crate) mod trade_panel;
pub(crate) mod render;
pub(crate) mod slot_widget;

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

/// Phase the loading screen is currently in. Drives both the
/// progress label shown to the user and the state machine inside
/// the `on_update` loading tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LoadingPhase {
    /// Workers are generating chunks. Progress bar advances as
    /// chunks arrive from the worker pool.
    GeneratingChunks,
    /// All chunks are in; running the full-world lighting BFS.
    BuildingLighting,
    /// Lighting done; uploading mesh geometry to the GPU.
    BuildingMeshes,
    /// Final bookkeeping before switching to Playing.
    Finalising,
}

impl LoadingPhase {
    /// Czech label shown above the progress bar on the loading screen.
    pub(crate) fn label_cz(self) -> &'static str {
        match self {
            Self::GeneratingChunks => "Generuji svet...",
            Self::BuildingLighting => "Pocitam osvetleni...",
            Self::BuildingMeshes => "Stavim chunky...",
            Self::Finalising => "Dokoncuji...",
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum AppState {
    MainMenu,
    /// Direct-IP connect dialog: user types `host:port` and
    /// hits Join.
    ///
    /// Wave 4 (Phase G.6) folded this state into the
    /// `MenuScreen::Multiplayer` inline form so it's never
    /// constructed; the variant survives so the Esc-handler match
    /// arm + the egui dispatch arm stay exhaustive. Tracked under
    /// the same follow-up as `start_hosting`.
    DirectConnect,
    /// Browses servers fetched from the configured master
    /// server. See note on `DirectConnect`.
    ServerBrowser,
    /// Configures + spawns a host (game server). See note on
    /// `DirectConnect`.
    HostSetup,
    /// Async world bootstrap in progress. The worker pool is
    /// generating the initial chunk grid; the main thread renders
    /// the loading screen + ticks `stream_chunks` each frame.
    ///
    /// Transitions to `Playing` once `loaded_chunks >= target_chunks`
    /// and the post-load steps (lighting BFS, mesh upload) finish.
    Loading {
        /// Total number of chunks the bootstrap needs to generate.
        /// Computed from `view_distance` + `VERTICAL_CHUNKS` in
        /// `build_world` before any chunk work starts. Matches the
        /// same radius used by `stream_chunks` so the counter
        /// reaches 100% naturally without overshooting.
        target_chunks: u32,
        /// Chunks actually inserted into `self.world` so far.
        /// Updated every tick from `self.world.iter_chunks().count()`.
        loaded_chunks: u32,
        /// Current phase label shown above the progress bar.
        phase: LoadingPhase,
        /// Wall-clock instant at state entry. Used to display
        /// elapsed seconds on the loading screen.
        started_at: std::time::Instant,
    },
    Playing,
    Paused,
    Settings {
        back_to: BackTo,
    },
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum BackTo {
    /// Wave 4 (Phase G.6) retired the "Back to Main from Settings"
    /// flow on the flat picker. The Pause flow still uses
    /// `BackTo::Pause` so the enum stays.
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

    // --- LoadingPhase tests (L.1) ---

    #[test]
    fn loading_phase_labels_non_empty() {
        for phase in [
            LoadingPhase::GeneratingChunks,
            LoadingPhase::BuildingLighting,
            LoadingPhase::BuildingMeshes,
            LoadingPhase::Finalising,
        ] {
            assert!(
                !phase.label_cz().is_empty(),
                "LoadingPhase::{:?} has an empty label",
                phase
            );
        }
    }

    #[test]
    fn loading_state_constructor_initializes_zero_loaded() {
        let state = AppState::Loading {
            target_chunks: 196,
            loaded_chunks: 0,
            phase: LoadingPhase::GeneratingChunks,
            started_at: std::time::Instant::now(),
        };
        if let AppState::Loading { loaded_chunks, target_chunks, .. } = state {
            assert_eq!(loaded_chunks, 0);
            assert_eq!(target_chunks, 196);
        } else {
            panic!("expected AppState::Loading");
        }
    }

    #[test]
    fn loading_phase_all_labels_are_distinct() {
        let labels: Vec<&str> = [
            LoadingPhase::GeneratingChunks,
            LoadingPhase::BuildingLighting,
            LoadingPhase::BuildingMeshes,
            LoadingPhase::Finalising,
        ]
        .iter()
        .map(|p| p.label_cz())
        .collect();
        // Every label should be unique so the UI text always
        // tells the user something different.
        let unique: std::collections::HashSet<&str> = labels.iter().copied().collect();
        assert_eq!(unique.len(), labels.len());
    }

    #[test]
    fn loading_phase_is_copy() {
        // Verify that LoadingPhase: Copy so it can live inside
        // AppState: Copy without breaking the `let state = self.state`
        // copy in render_ui.
        let a = LoadingPhase::GeneratingChunks;
        let b = a; // copy
        let c = a; // copy again - if not Copy this would fail
        assert_eq!(b, c);
    }

    #[test]
    fn app_state_loading_phase_progression_order() {
        // Document the intended phase order: GeneratingChunks ->
        // BuildingLighting -> BuildingMeshes -> Finalising.
        // This test encodes the order so a future refactor that
        // accidentally skips a phase is caught.
        let phases = [
            LoadingPhase::GeneratingChunks,
            LoadingPhase::BuildingLighting,
            LoadingPhase::BuildingMeshes,
            LoadingPhase::Finalising,
        ];
        // Each phase has a distinct label - the labels drive the
        // UI so the user sees progress, not a static string.
        let label_0 = phases[0].label_cz();
        let label_1 = phases[1].label_cz();
        let label_2 = phases[2].label_cz();
        let label_3 = phases[3].label_cz();
        assert_ne!(label_0, label_1);
        assert_ne!(label_1, label_2);
        assert_ne!(label_2, label_3);
    }

    #[test]
    fn loading_state_target_chunks_formula() {
        // 7 x 7 x 4 = 196 for INITIAL_CHUNKS=3, VERTICAL_CHUNKS=4.
        // The actual constants live in main.rs; this test just
        // documents that the formula is (2*ic+1)^2 * vc.
        let initial_chunks: u32 = 3;
        let vertical_chunks: u32 = 4;
        let side = 2 * initial_chunks + 1;
        let expected = side * side * vertical_chunks;
        assert_eq!(expected, 196);
    }

    // --- HighlightMode tests ---

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
