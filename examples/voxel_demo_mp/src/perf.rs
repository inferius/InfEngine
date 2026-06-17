//! W198 - last per-phase perf snapshot, shared so the F3 debug overlay
//! can show the ms breakdown on screen.
//!
//! The perf diag in [`crate::app_on_update`] already computes the
//! per-phase costs (render / sim / stream / liquids / drawq / mobs) every
//! ~2.5s, but only logs them via `tracing`. The user iterates by
//! screenshotting the HUD rather than reading the console, so we also
//! stash the latest values here (ms x100, packed into an `AtomicU32`)
//! for the overlay to read. Diagnostic only - never on the hot path.

use std::sync::atomic::{AtomicU32, Ordering};

pub(crate) static RENDER_MS: AtomicU32 = AtomicU32::new(0);
pub(crate) static SIM_MS: AtomicU32 = AtomicU32::new(0);
pub(crate) static STREAM_MS: AtomicU32 = AtomicU32::new(0);
pub(crate) static LIQUIDS_MS: AtomicU32 = AtomicU32::new(0);
pub(crate) static DRAWQ_MS: AtomicU32 = AtomicU32::new(0);
pub(crate) static MOBS_MS: AtomicU32 = AtomicU32::new(0);

/// Store a ms value (kept to 0.01 ms resolution).
pub(crate) fn store(field: &AtomicU32, ms: f32) {
    field.store((ms.max(0.0) * 100.0) as u32, Ordering::Relaxed);
}

/// Read a ms value back.
pub(crate) fn load(field: &AtomicU32) -> f32 {
    field.load(Ordering::Relaxed) as f32 / 100.0
}
