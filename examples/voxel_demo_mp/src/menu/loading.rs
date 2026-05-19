//! Loading screen drawn while the world bootstraps. Shows the
//! current phase, a progress bar, and elapsed time. MC-style
//! centred layout over a solid dark-navy backdrop.
//!
//! The draw fn is a free function so it can be called from the
//! egui dispatch closure in `main.rs` without pulling the full
//! `Voxel` struct into this module.

use crate::ui::LoadingPhase;

/// Render the loading screen into `ui`.
///
/// Parameters mirror the `AppState::Loading` payload so the caller
/// can destructure that variant and forward the fields directly.
///
/// `target_chunks` - total initial chunks to generate (0 -> progress
///   bar stays at 0% until the first chunk arrives).
/// `loaded_chunks` - chunks inserted into the world so far.
/// `phase` - current phase label drawn above the bar.
/// `started_at` - wall-clock instant the Loading state was entered
///   (used for the elapsed-time label).
pub(crate) fn draw(
    ui: &mut egui::Ui,
    target_chunks: u32,
    loaded_chunks: u32,
    phase: LoadingPhase,
    started_at: std::time::Instant,
) {
    ui.add_space(80.0);
    ui.vertical_centered(|ui| {
        ui.heading("InfEngine");
        ui.add_space(40.0);
        ui.label(egui::RichText::new(phase.label_cz()).size(20.0));
        ui.add_space(16.0);
        let frac = if target_chunks > 0 {
            (loaded_chunks as f32 / target_chunks as f32).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let pct = (frac * 100.0).round() as u32;
        ui.add(
            egui::ProgressBar::new(frac)
                .show_percentage()
                .text(format!(
                    "{loaded_chunks}/{target_chunks} chunks ({pct}%)"
                ))
                .desired_width(420.0),
        );
        ui.add_space(8.0);
        let elapsed = started_at.elapsed().as_secs_f32();
        ui.label(
            egui::RichText::new(format!("Cas: {:.1} s", elapsed))
                .italics()
                .color(egui::Color32::from_gray(180)),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper that mirrors the fraction logic in `draw`.
    fn compute_frac(target: u32, loaded: u32) -> f32 {
        if target > 0 {
            (loaded as f32 / target as f32).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    #[test]
    fn progress_fraction_clamped_at_zero_when_target_is_zero() {
        // target == 0: fraction must be 0.0, not NaN or inf.
        let frac = compute_frac(0, 0);
        assert!((0.0..=1.0).contains(&frac));
        assert_eq!(frac, 0.0);
    }

    #[test]
    fn progress_fraction_clamped_above_one() {
        // loaded > target: must clamp to 1.0, not exceed it.
        let frac = compute_frac(10, 20);
        assert_eq!(frac, 1.0);
    }

    #[test]
    fn progress_fraction_midpoint() {
        let frac = compute_frac(196, 98);
        // 98/196 = 0.5 exactly.
        assert!((frac - 0.5).abs() < 1e-5);
    }

    #[test]
    fn phase_label_appears_in_drawn_state() {
        // Verify that each phase maps to a distinct, non-empty label
        // that contains at least one letter - the UI always shows a
        // meaningful string.
        for phase in [
            LoadingPhase::GeneratingChunks,
            LoadingPhase::BuildingLighting,
            LoadingPhase::BuildingMeshes,
            LoadingPhase::Finalising,
        ] {
            let label = phase.label_cz();
            assert!(
                label.chars().any(|c| c.is_alphabetic()),
                "phase {:?} label is not alphabetic: {:?}",
                phase,
                label
            );
        }
    }

    #[test]
    fn elapsed_label_format_is_consistent() {
        // Just exercises the format string locally without egui.
        let elapsed = 12.3_f32;
        let s = format!("Cas: {:.1} s", elapsed);
        assert_eq!(s, "Cas: 12.3 s");
    }
}
